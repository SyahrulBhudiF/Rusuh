//! Full model registry with reference counting, quota tracking, suspension,
//! and handler-type conversion — mirrors Go `registry.ModelRegistry`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::RwLock;
use tracing::debug;

use super::model_info::ExtModelInfo;
use super::static_models;

const QUOTA_EXPIRED_DURATION: Duration = Duration::from_secs(300); // 5 minutes

// ── Registration ─────────────────────────────────────────────────────────────

struct ModelRegistration {
    info: ExtModelInfo,
    info_by_provider: HashMap<String, ExtModelInfo>,
    count: i32,
    last_updated: Instant,
    quota_exceeded_clients: HashMap<String, Instant>,
    providers: HashMap<String, i32>,
    suspended_clients: HashMap<String, String>,
}

// ── Registry ─────────────────────────────────────────────────────────────────

pub struct ModelRegistry {
    models: RwLock<HashMap<String, ModelRegistration>>,
    client_models: RwLock<HashMap<String, Vec<String>>>,
    client_model_infos: RwLock<HashMap<String, HashMap<String, ExtModelInfo>>>,
    client_providers: RwLock<HashMap<String, String>>,
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: RwLock::new(HashMap::new()),
            client_models: RwLock::new(HashMap::new()),
            client_model_infos: RwLock::new(HashMap::new()),
            client_providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a client and its models.
    pub async fn register_client(
        &self,
        client_id: &str,
        client_provider: &str,
        models: Vec<ExtModelInfo>,
    ) {
        let provider = client_provider.to_lowercase();
        let now = Instant::now();

        let mut unique_ids: Vec<String> = Vec::new();
        let mut raw_ids: Vec<String> = Vec::new();
        let mut new_models: HashMap<String, ExtModelInfo> = HashMap::new();
        let mut new_counts: HashMap<String, i32> = HashMap::new();

        for model in &models {
            if model.id.is_empty() {
                continue;
            }
            raw_ids.push(model.id.clone());
            *new_counts.entry(model.id.clone()).or_insert(0) += 1;
            new_models.entry(model.id.clone()).or_insert_with(|| model.clone());
            if !unique_ids.contains(&model.id) {
                unique_ids.push(model.id.clone());
            }
        }

        if unique_ids.is_empty() {
            self.unregister_client(client_id).await;
            return;
        }

        let had_existing = self.client_models.read().await.contains_key(client_id);

        if !had_existing {
            // Pure addition
            let mut reg = self.models.write().await;
            for model_id in &raw_ids {
                let model = &new_models[model_id];
                Self::add_registration(&mut reg, model_id, &provider, model, now);
            }
            drop(reg);
            let model_count = raw_ids.len();
            self.client_models
                .write()
                .await
                .insert(client_id.to_string(), raw_ids);

            let mut infos: HashMap<String, ExtModelInfo> = HashMap::new();
            for (id, m) in &new_models {
                infos.insert(id.clone(), m.clone());
            }
            self.client_model_infos
                .write()
                .await
                .insert(client_id.to_string(), infos);

            if !provider.is_empty() {
                self.client_providers
                    .write()
                    .await
                    .insert(client_id.to_string(), provider.clone());
            }

            debug!(
                "registered client {} from provider {} with {} models",
                client_id,
                client_provider,
                model_count
            );
            return;
        }

        // Reconciliation path (update existing client)
        let old_models = self
            .client_models
            .read()
            .await
            .get(client_id)
            .cloned()
            .unwrap_or_default();
        let old_provider = self
            .client_providers
            .read()
            .await
            .get(client_id)
            .cloned()
            .unwrap_or_default();

        let mut old_counts: HashMap<String, i32> = HashMap::new();
        for id in &old_models {
            *old_counts.entry(id.clone()).or_insert(0) += 1;
        }

        // Removals
        let mut reg = self.models.write().await;
        for (id, &old_c) in &old_counts {
            if new_counts.get(id).copied().unwrap_or(0) == 0 {
                for _ in 0..old_c {
                    Self::remove_registration(&mut reg, client_id, id, &old_provider, now);
                }
            }
        }

        // Additions
        for (id, &new_c) in &new_counts {
            let old_c = old_counts.get(id).copied().unwrap_or(0);
            if new_c > old_c {
                let model = &new_models[id];
                for _ in 0..(new_c - old_c) {
                    Self::add_registration(&mut reg, id, &provider, model, now);
                }
            }
        }

        // Update metadata
        for id in &unique_ids {
            if let Some(registration) = reg.get_mut(id) {
                registration.info = new_models[id].clone();
                if !provider.is_empty() {
                    registration
                        .info_by_provider
                        .insert(provider.clone(), new_models[id].clone());
                }
                registration.last_updated = now;
                registration.quota_exceeded_clients.remove(client_id);
                registration.suspended_clients.remove(client_id);
            }
        }
        drop(reg);

        self.client_models
            .write()
            .await
            .insert(client_id.to_string(), raw_ids);

        let mut infos: HashMap<String, ExtModelInfo> = HashMap::new();
        for (id, m) in &new_models {
            infos.insert(id.clone(), m.clone());
        }
        self.client_model_infos
            .write()
            .await
            .insert(client_id.to_string(), infos);

        if !provider.is_empty() {
            self.client_providers
                .write()
                .await
                .insert(client_id.to_string(), provider);
        }

        debug!("reconciled client {} models", client_id);
    }

    /// Unregister a client and decrement model counts.
    pub async fn unregister_client(&self, client_id: &str) {
        let models = self
            .client_models
            .write()
            .await
            .remove(client_id)
            .unwrap_or_default();
        let provider = self
            .client_providers
            .write()
            .await
            .remove(client_id)
            .unwrap_or_default();
        self.client_model_infos.write().await.remove(client_id);

        let now = Instant::now();
        let mut reg = self.models.write().await;
        for model_id in &models {
            Self::remove_registration(&mut reg, client_id, model_id, &provider, now);
        }

        debug!("unregistered client {}", client_id);
    }

    /// Mark model as quota exceeded for client.
    pub async fn set_quota_exceeded(&self, client_id: &str, model_id: &str) {
        if let Some(reg) = self.models.write().await.get_mut(model_id) {
            reg.quota_exceeded_clients
                .insert(client_id.to_string(), Instant::now());
            debug!("marked {} quota exceeded for {}", model_id, client_id);
        }
    }

    /// Clear quota exceeded for client/model.
    pub async fn clear_quota_exceeded(&self, client_id: &str, model_id: &str) {
        if let Some(reg) = self.models.write().await.get_mut(model_id) {
            reg.quota_exceeded_clients.remove(client_id);
        }
    }

    /// Suspend a client for a model.
    pub async fn suspend_client_model(&self, client_id: &str, model_id: &str, reason: &str) {
        if let Some(reg) = self.models.write().await.get_mut(model_id) {
            if reg.suspended_clients.contains_key(client_id) {
                return;
            }
            reg.suspended_clients
                .insert(client_id.to_string(), reason.to_string());
            reg.last_updated = Instant::now();
            debug!("suspended {} for model {}: {}", client_id, model_id, reason);
        }
    }

    /// Resume a client for a model.
    pub async fn resume_client_model(&self, client_id: &str, model_id: &str) {
        if let Some(reg) = self.models.write().await.get_mut(model_id) {
            if reg.suspended_clients.remove(client_id).is_some() {
                reg.last_updated = Instant::now();
                debug!("resumed {} for model {}", client_id, model_id);
            }
        }
    }

    /// Get effective count of available clients for a model.
    pub async fn get_model_count(&self, model_id: &str) -> i32 {
        let reg = self.models.read().await;
        match reg.get(model_id) {
            Some(r) => Self::effective_count(r),
            None => 0,
        }
    }

    /// Get available models formatted for a handler type.
    pub async fn get_available_models(&self, handler_type: &str) -> Vec<Value> {
        let reg = self.models.read().await;
        let mut out = Vec::new();

        for registration in reg.values() {
            let eff = Self::effective_count(registration);
            let avail = registration.count;
            let expired = Self::expired_quota_count(registration);
            let cooldown = Self::cooldown_suspended_count(registration);
            let other_suspended = Self::other_suspended_count(registration);

            if eff > 0
                || (avail > 0 && (expired > 0 || cooldown > 0) && other_suspended == 0)
            {
                if let Some(v) = Self::convert_model(&registration.info, handler_type) {
                    out.push(v);
                }
            }
        }
        out
    }

    /// Get model info, preferring provider-specific definition.
    pub async fn get_model_info(
        &self,
        model_id: &str,
        provider: &str,
    ) -> Option<ExtModelInfo> {
        let reg = self.models.read().await;
        if let Some(r) = reg.get(model_id) {
            if !provider.is_empty() {
                if let Some(info) = r.info_by_provider.get(provider) {
                    return Some(info.clone());
                }
            }
            return Some(r.info.clone());
        }
        None
    }

    /// Lookup model info: dynamic registry first, then static definitions.
    pub async fn lookup_model_info(
        &self,
        model_id: &str,
        provider: &str,
    ) -> Option<ExtModelInfo> {
        if let Some(info) = self.get_model_info(model_id, provider).await {
            return Some(info);
        }
        static_models::lookup_static_model(model_id)
    }

    /// Get providers that supply a model, ordered by count desc.
    pub async fn get_model_providers(&self, model_id: &str) -> Vec<String> {
        let reg = self.models.read().await;
        let Some(r) = reg.get(model_id) else {
            return vec![];
        };
        let mut providers: Vec<(String, i32)> = r
            .providers
            .iter()
            .filter(|(_, &c)| c > 0)
            .map(|(k, &v)| (k.clone(), v))
            .collect();
        providers.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        providers.into_iter().map(|(k, _)| k).collect()
    }

    /// Cleanup expired quota entries.
    pub async fn cleanup_expired_quotas(&self) {
        let mut reg = self.models.write().await;
        for registration in reg.values_mut() {
            registration
                .quota_exceeded_clients
                .retain(|_, t| t.elapsed() < QUOTA_EXPIRED_DURATION);
        }
    }

    /// Check if client supports a model.
    pub async fn client_supports_model(&self, client_id: &str, model_id: &str) -> bool {
        self.client_models
            .read()
            .await
            .get(client_id)
            .map(|ids| ids.iter().any(|id| id.eq_ignore_ascii_case(model_id)))
            .unwrap_or(false)
    }

    // ── Internal ─────────────────────────────────────────────────────────

    fn add_registration(
        reg: &mut HashMap<String, ModelRegistration>,
        model_id: &str,
        provider: &str,
        model: &ExtModelInfo,
        now: Instant,
    ) {
        if let Some(existing) = reg.get_mut(model_id) {
            existing.count += 1;
            existing.info = model.clone();
            existing.last_updated = now;
            if !provider.is_empty() {
                *existing.providers.entry(provider.to_string()).or_insert(0) += 1;
                existing
                    .info_by_provider
                    .insert(provider.to_string(), model.clone());
            }
        } else {
            let mut providers = HashMap::new();
            let mut info_by_provider = HashMap::new();
            if !provider.is_empty() {
                providers.insert(provider.to_string(), 1);
                info_by_provider.insert(provider.to_string(), model.clone());
            }
            reg.insert(
                model_id.to_string(),
                ModelRegistration {
                    info: model.clone(),
                    info_by_provider,
                    count: 1,
                    last_updated: now,
                    quota_exceeded_clients: HashMap::new(),
                    providers,
                    suspended_clients: HashMap::new(),
                },
            );
        }
    }

    fn remove_registration(
        reg: &mut HashMap<String, ModelRegistration>,
        client_id: &str,
        model_id: &str,
        provider: &str,
        now: Instant,
    ) {
        let Some(registration) = reg.get_mut(model_id) else {
            return;
        };
        registration.count -= 1;
        registration.last_updated = now;
        registration.quota_exceeded_clients.remove(client_id);
        registration.suspended_clients.remove(client_id);

        if registration.count < 0 {
            registration.count = 0;
        }

        if !provider.is_empty() {
            if let Some(c) = registration.providers.get_mut(provider) {
                *c -= 1;
                if *c <= 0 {
                    registration.providers.remove(provider);
                    registration.info_by_provider.remove(provider);
                }
            }
        }

        if registration.count <= 0 {
            reg.remove(model_id);
        }
    }

    fn effective_count(r: &ModelRegistration) -> i32 {
        let expired = Self::expired_quota_count(r);
        let other_suspended = Self::other_suspended_count(r);
        let eff = r.count - expired - other_suspended;
        if eff < 0 { 0 } else { eff }
    }

    fn expired_quota_count(r: &ModelRegistration) -> i32 {
        r.quota_exceeded_clients
            .values()
            .filter(|t| t.elapsed() < QUOTA_EXPIRED_DURATION)
            .count() as i32
    }

    fn cooldown_suspended_count(r: &ModelRegistration) -> i32 {
        r.suspended_clients
            .values()
            .filter(|reason| reason.eq_ignore_ascii_case("quota"))
            .count() as i32
    }

    fn other_suspended_count(r: &ModelRegistration) -> i32 {
        r.suspended_clients
            .values()
            .filter(|reason| !reason.eq_ignore_ascii_case("quota"))
            .count() as i32
    }

    fn convert_model(model: &ExtModelInfo, handler_type: &str) -> Option<Value> {
        match handler_type {
            "openai" => {
                let mut r = json!({
                    "id": model.id,
                    "object": "model",
                    "owned_by": model.owned_by,
                });
                if model.created > 0 { r["created"] = json!(model.created); }
                if !model.provider_type.is_empty() { r["type"] = json!(model.provider_type); }
                if let Some(ref d) = model.display_name { r["display_name"] = json!(d); }
                if let Some(ref v) = model.version { r["version"] = json!(v); }
                if let Some(ref d) = model.description { r["description"] = json!(d); }
                if model.context_length > 0 { r["context_length"] = json!(model.context_length); }
                if model.max_completion_tokens > 0 { r["max_completion_tokens"] = json!(model.max_completion_tokens); }
                if !model.supported_parameters.is_empty() { r["supported_parameters"] = json!(model.supported_parameters); }
                Some(r)
            }
            "claude" => {
                let mut r = json!({
                    "id": model.id,
                    "object": "model",
                    "owned_by": model.owned_by,
                });
                if model.created > 0 { r["created_at"] = json!(model.created); }
                r["type"] = json!("model");
                if let Some(ref d) = model.display_name { r["display_name"] = json!(d); }
                Some(r)
            }
            "gemini" => {
                let mut r = json!({});
                r["name"] = json!(model.name.as_deref().unwrap_or(&model.id));
                if let Some(ref v) = model.version { r["version"] = json!(v); }
                if let Some(ref d) = model.display_name { r["displayName"] = json!(d); }
                if let Some(ref d) = model.description { r["description"] = json!(d); }
                if model.input_token_limit > 0 { r["inputTokenLimit"] = json!(model.input_token_limit); }
                if model.output_token_limit > 0 { r["outputTokenLimit"] = json!(model.output_token_limit); }
                if !model.supported_generation_methods.is_empty() { r["supportedGenerationMethods"] = json!(model.supported_generation_methods); }
                Some(r)
            }
            _ => {
                let mut r = json!({
                    "id": model.id,
                    "object": "model",
                });
                if !model.owned_by.is_empty() { r["owned_by"] = json!(model.owned_by); }
                if !model.provider_type.is_empty() { r["type"] = json!(model.provider_type); }
                if model.created > 0 { r["created"] = json!(model.created); }
                Some(r)
            }
        }
    }
}

