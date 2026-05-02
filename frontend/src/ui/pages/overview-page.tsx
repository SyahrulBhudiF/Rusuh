import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useMemo } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

import { queryKeys, useOverviewQuery } from "../../lib/query";
import { PageShell } from "../page-shell";
import { QueryState } from "../query-state";
import { statusTone } from "../status-tone";

export function OverviewPage() {
  const queryClient = useQueryClient();
  const overview = useOverviewQuery();
  const accountSummaries = overview.data?.account_summaries;
  const providerNames = overview.data?.provider_names ?? [];

  const summaryRows = useMemo(
    () =>
      (accountSummaries ?? []).map((summary) => ({
        ...summary,
        chips: [
          ["active", summary.active],
          ["refreshing", summary.refreshing],
          ["pending", summary.pending],
          ["error", summary.error],
          ["disabled", summary.disabled],
          ["unknown", summary.unknown],
        ].filter(([, count]) => Number(count) > 0),
      })),
    [accountSummaries],
  );

  const providerCards = useMemo(
    () =>
      summaryRows.map((summary) => ({
        name: summary.provider,
        total: summary.total,
        active: summary.active > 0,
      })),
    [summaryRows],
  );

  const hasProviders = providerNames.length > 0;

  return (
    <PageShell
      eyebrow="Dashboard"
      title="Runtime Overview"
      description="Monitor system health, manage providers, and control proxy infrastructure from one unified dashboard."
      actions={
        <Button
          type="button"
          variant="outline"
          onClick={() => {
            void queryClient.invalidateQueries({
              queryKey: queryKeys.overview,
            });
          }}
          className="h-11 rounded-full px-5"
        >
          Refresh
        </Button>
      }
    >
      <QueryState
        isLoading={overview.isLoading}
        isError={overview.isError}
        error={overview.error as Error | null}
      >
        {overview.data ? (
          <>
            {!hasProviders ? (
              <section className="dashboard-panel mb-4 rounded-3xl p-5">
                <p className="text-sm font-medium">Start here</p>
                <p className="text-muted-foreground mt-2 text-sm">
                  Add an account, then come back here to confirm the runtime is
                  healthy.
                </p>
                <div className="mt-4 flex flex-wrap gap-2">
                  <Button asChild className="rounded-full px-5">
                    <Link to="/accounts/add">Add Account</Link>
                  </Button>
                </div>
              </section>
            ) : null}

            <div className="grid gap-8">
              <section className="space-y-6">
                <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-4">
                  {overview.data.cards.map((card) => (
                    <div key={card.label} className="dashboard-panel rounded-2xl p-4">
                      <p className="text-muted-foreground text-xs uppercase tracking-[0.2em]">
                        {card.label}
                      </p>
                      <p className="text-foreground mt-3 text-3xl font-semibold">
                        {card.value}
                      </p>
                      <p className="text-muted-foreground mt-2 text-sm leading-6">
                        {card.hint}
                      </p>
                    </div>
                  ))}
                </div>

                <div className="grid gap-7 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
                  <section className="space-y-4">
                    <div className="dashboard-panel rounded-2xl p-5">
                      <div>
                        <h3 className="text-lg font-semibold">Configuration</h3>
                        <p className="text-muted-foreground mt-1 text-sm">
                          Routing strategy, service name, and model availability.
                        </p>
                      </div>
                      <div className="mt-4 grid gap-3">
                        <div className="dashboard-panel flex items-center justify-between rounded-2xl px-4 py-3 text-sm">
                          <div>
                            <p className="text-muted-foreground text-xs uppercase tracking-[0.18em]">
                              Routing Strategy
                            </p>
                            <p className="text-foreground mt-1 font-medium">
                              {overview.data.routing_strategy}
                            </p>
                          </div>
                        </div>
                        <div className="dashboard-panel flex items-center justify-between rounded-2xl px-4 py-3 text-sm">
                          <div>
                            <p className="text-muted-foreground text-xs uppercase tracking-[0.18em]">
                              Service Name
                            </p>
                            <p className="text-foreground mt-1 font-medium">
                              {overview.data.health.service}
                            </p>
                          </div>
                        </div>
                        <div className="dashboard-panel flex items-center justify-between rounded-2xl px-4 py-3 text-sm">
                          <div>
                            <p className="text-muted-foreground text-xs uppercase tracking-[0.18em]">
                              Active Models
                            </p>
                            <p className="text-foreground mt-1 font-medium">
                              {overview.data.available_model_count}
                            </p>
                          </div>
                        </div>
                      </div>
                    </div>

                    <div className="dashboard-panel rounded-2xl p-5">
                      <div className="flex items-center justify-between">
                        <div>
                          <h3 className="text-lg font-semibold">Connected Providers</h3>
                          <p className="text-muted-foreground text-sm">
                            Live provider connectivity.
                          </p>
                        </div>
                        <Badge
                          variant="outline"
                          className="dashboard-status w-fit rounded-full px-3 py-1 text-xs"
                        >
                          {providerNames.length} provider(s)
                        </Badge>
                      </div>
                      <div className="mt-4 space-y-2">
                        {providerNames.length > 0 ? (
                          providerNames.map((name) => (
                            <div
                              key={name}
                              className="dashboard-panel flex items-center justify-between rounded-2xl px-4 py-3 text-sm"
                            >
                              <span className="text-foreground">{name}</span>
                              <span className="text-emerald-400">Online</span>
                            </div>
                          ))
                        ) : (
                          <div className="text-muted-foreground text-sm">
                            No providers yet.
                          </div>
                        )}
                      </div>
                    </div>
                  </section>

                  <section className="space-y-4">
                    <div className="flex items-center justify-between">
                      <div>
                          <h3 className="text-lg font-semibold">Provider Details</h3>
                        <p className="text-muted-foreground text-sm">
                          Status and connected accounts by provider.
                        </p>
                      </div>
                      <Badge
                        variant="outline"
                        className="dashboard-status w-fit rounded-full px-3 py-1 text-xs"
                      >
                        {providerCards.length} provider(s)
                      </Badge>
                    </div>
                    {providerCards.length > 0 ? (
                      <div className="grid gap-3 sm:grid-cols-2">
                        {providerCards.map((provider) => (
                          <div key={provider.name} className="dashboard-panel rounded-2xl p-4">
                            <div className="flex items-start justify-between gap-3">
                              <div>
                                <p className="text-foreground font-medium">{provider.name}</p>
                                <p className="text-muted-foreground mt-1 text-sm">
                                  Accounts: {provider.total}
                                </p>
                              </div>
                              <Badge
                                variant="outline"
                                className={`rounded-full px-2.5 py-1 text-xs ${statusTone(provider.active ? "active" : "disabled")}`}
                              >
                                {provider.active ? "Active" : "Inactive"}
                              </Badge>
                            </div>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="dashboard-panel text-muted-foreground rounded-2xl p-4 text-sm">
                        No providers configured yet.
                      </div>
                    )}
                  </section>
                </div>
              </section>
            </div>
          </>
        ) : null}
      </QueryState>
    </PageShell>
  );
}
