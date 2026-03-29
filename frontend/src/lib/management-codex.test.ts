import { afterEach, describe, expect, mock, test } from 'bun:test'

import { checkCodexQuota } from './management-codex'

const originalFetch = globalThis.fetch

function jsonResponse(body: unknown) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: {
      'Content-Type': 'application/json',
    },
  })
}

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe('management-codex', () => {
  test('checkCodexQuota posts quota request with management key and name', async () => {
    const fetchMock = mock(async () =>
      jsonResponse({
        account: 'codex-user.json',
        status: 'available',
        upstream_status: 200,
        plan_type: 'team',
        rate_limit: {
          allowed: true,
          limit_reached: false,
          primary_window: {
            used_percent: 5,
            limit_window_seconds: 18000,
            reset_after_seconds: 15247,
            reset_at: 1774816656,
          },
          secondary_window: {
            used_percent: 9,
            limit_window_seconds: 604800,
            reset_after_seconds: 583764,
            reset_at: 1775385173,
          },
        },
        code_review_rate_limit: {
          allowed: true,
          limit_reached: false,
          primary_window: {
            used_percent: 0,
            limit_window_seconds: 604800,
            reset_after_seconds: 604800,
            reset_at: 1775406209,
          },
          secondary_window: null,
        },
        credits: {
          has_credits: false,
          unlimited: false,
          balance: null,
          approx_local_messages: null,
          approx_cloud_messages: null,
        },
        spend_control: {
          reached: false,
        },
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await checkCodexQuota('quota-secret', { name: 'codex-user.json' })

    expect(response).toEqual({
      account: 'codex-user.json',
      status: 'available',
      upstream_status: 200,
      plan_type: 'team',
      rate_limit: {
        allowed: true,
        limit_reached: false,
        primary_window: {
          used_percent: 5,
          limit_window_seconds: 18000,
          reset_after_seconds: 15247,
          reset_at: 1774816656,
        },
        secondary_window: {
          used_percent: 9,
          limit_window_seconds: 604800,
          reset_after_seconds: 583764,
          reset_at: 1775385173,
        },
      },
      code_review_rate_limit: {
        allowed: true,
        limit_reached: false,
        primary_window: {
          used_percent: 0,
          limit_window_seconds: 604800,
          reset_after_seconds: 604800,
          reset_at: 1775406209,
        },
        secondary_window: null,
      },
      credits: {
        has_credits: false,
        unlimited: false,
        balance: null,
        approx_local_messages: null,
        approx_cloud_messages: null,
      },
      spend_control: {
        reached: false,
      },
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/v0/management/codex/check-quota')
    expect(init.method).toBe('POST')
    expect(init.headers).toEqual({
      Accept: 'application/json',
      'X-Management-Key': 'quota-secret',
      'Content-Type': 'application/json',
    })
    expect(init.body).toBe(JSON.stringify({ name: 'codex-user.json' }))
  })
})
