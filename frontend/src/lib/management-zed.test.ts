import { afterEach, describe, expect, mock, test } from 'bun:test'

import { checkZedQuota, fetchZedLoginStatus, fetchZedModels, startZedLogin } from './management-zed'

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

describe('management-zed', () => {
  test('startZedLogin posts initiate request with trimmed name', async () => {
    const fetchMock = mock(async () =>
      jsonResponse({
        status: 'waiting',
        session_id: 'session-1',
        login_url: 'https://zed.dev/native_app_signin?native_app_port=1234',
        port: 1234,
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await startZedLogin('test-secret', { name: '  work-zed  ' })

    expect(response).toEqual({
      status: 'waiting',
      session_id: 'session-1',
      login_url: 'https://zed.dev/native_app_signin?native_app_port=1234',
      port: 1234,
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/v0/management/zed/login/initiate')
    expect(init.method).toBe('POST')
    expect(init.headers).toEqual({
      Accept: 'application/json',
      'X-Management-Key': 'test-secret',
      'Content-Type': 'application/json',
    })
    expect(init.body).toBe(JSON.stringify({ name: 'work-zed' }))
  })

  test('fetchZedLoginStatus requests session status with management key', async () => {
    const fetchMock = mock(async () =>
      jsonResponse({
        status: 'completed',
        session_id: 'session-2',
        filename: 'zed-user.json',
        user_id: 'user-1',
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await fetchZedLoginStatus('status-secret', 'session with spaces')

    expect(response).toEqual({
      status: 'completed',
      session_id: 'session-2',
      filename: 'zed-user.json',
      user_id: 'user-1',
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/v0/management/zed/login/status?session_id=session%20with%20spaces')
    expect(init?.headers).toEqual({
      Accept: 'application/json',
      'X-Management-Key': 'status-secret',
    })
  })

  test('checkZedQuota posts quota request with management key', async () => {
    const fetchMock = mock(async () =>
      jsonResponse({
        account: 'zed-user.json',
        status: 'available',
        plan: 'pro',
        model_requests_used: 12,
        model_requests_limit: 500,
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await checkZedQuota('quota-secret', { name: 'zed-user.json' })

    expect(response).toEqual({
      account: 'zed-user.json',
      status: 'available',
      plan: 'pro',
      model_requests_used: 12,
      model_requests_limit: 500,
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/v0/management/zed/check-quota')
    expect(init.method).toBe('POST')
    expect(init.headers).toEqual({
      Accept: 'application/json',
      'X-Management-Key': 'quota-secret',
      'Content-Type': 'application/json',
    })
    expect(init.body).toBe(JSON.stringify({ name: 'zed-user.json' }))
  })

  test('fetchZedModels posts model request with management key', async () => {
    const fetchMock = mock(async () =>
      jsonResponse({
        account: 'zed-user.json',
        provider_key: 'zed',
        models: ['claude-sonnet-4.5', 'claude-opus-4.5'],
      }),
    )
    globalThis.fetch = fetchMock as unknown as typeof fetch

    const response = await fetchZedModels('models-secret', { name: 'zed-user.json' })

    expect(response).toEqual({
      account: 'zed-user.json',
      provider_key: 'zed',
      models: ['claude-sonnet-4.5', 'claude-opus-4.5'],
    })
    expect(fetchMock).toHaveBeenCalledTimes(1)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/v0/management/zed/models')
    expect(init.method).toBe('POST')
    expect(init.headers).toEqual({
      Accept: 'application/json',
      'X-Management-Key': 'models-secret',
      'Content-Type': 'application/json',
    })
    expect(init.body).toBe(JSON.stringify({ name: 'zed-user.json' }))
  })
})
