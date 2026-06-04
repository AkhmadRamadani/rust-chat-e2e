import { LogLevel } from './DevLog';

export async function apiFetch(
  method: string,
  path: string,
  body: any,
  token: string,
  logFn: (level: LogLevel, msg: string) => void
) {
  const url = `/api${path}`;
  const hdrs: Record<string, string> = { 'Content-Type': 'application/json' };
  if (token) hdrs['Authorization'] = `Bearer ${token}`;

  logFn('info', `${method} ${url}`);
  try {
    const res = await fetch(url, {
      method,
      headers: hdrs,
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await res.text();
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    const dataStr = typeof data === 'string' ? data : JSON.stringify(data);
    logFn(res.ok ? 'ok' : 'err', `${res.status} ← ${dataStr.slice(0, 300)}`);
    return { ok: res.ok, status: res.status, data };
  } catch (e: any) {
    logFn('err', `Network error: ${e.message}`);
    return { ok: false, status: 0, data: null };
  }
}

export async function adminFetch(
  method: string,
  path: string,
  body: any,
  adminToken: string,
  logFn: (level: LogLevel, msg: string) => void
) {
  const url = `/api${path}`;
  const hdrs: Record<string, string> = { 'Content-Type': 'application/json' };
  if (adminToken) hdrs['Authorization'] = `Bearer ${adminToken}`;

  logFn('info', `[admin] ${method} ${url}`);
  try {
    const res = await fetch(url, {
      method,
      headers: hdrs,
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await res.text();
    let data;
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
    const dataStr = typeof data === 'string' ? data : JSON.stringify(data);
    logFn(res.ok ? 'ok' : 'err', `${res.status} ← ${dataStr.slice(0, 300)}`);
    return { ok: res.ok, status: res.status, data };
  } catch (e: any) {
    logFn('err', `Network error: ${e.message}`);
    return { ok: false, status: 0, data: null };
  }
}
