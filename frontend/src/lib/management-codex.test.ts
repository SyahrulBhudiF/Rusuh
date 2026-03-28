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
        plan_type: 'plus',
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await checkCodexQuota('quota-secret', { name: 'codex-user.json' })

    expect(response).toEqual({
      account: 'codex-user.json',
      status: 'available',
      upstream_status: 200,
      plan_type: 'plus',
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
