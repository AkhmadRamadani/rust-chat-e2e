import { ApiError, Tenant, TenantRegistration, SubmitRegistrationResponse, UsageMetrics } from './types';

const BASE = '/api';

async function apiFetch<T>(method: string, path: string, body?: unknown, token?: string): Promise<T> {
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (token) headers['Authorization'] = `Bearer ${token}`;

  const res = await fetch(`${BASE}${path}`, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!res.ok) {
    let errBody: any = {};
    try { errBody = await res.json(); } catch (e) {}
    throw new ApiError(
      res.status,
      errBody.error_code || 'unknown_error',
      errBody.message || 'An unknown error occurred',
      errBody.request_id
    );
  }

  // Handle 204 No Content or empty responses
  const text = await res.text();
  return text ? JSON.parse(text) : ({} as T);
}

export const api = {
  // Admin
  listTenants: (token: string) => apiFetch<Tenant[]>('GET', '/admin/tenants', undefined, token),
  createTenant: (token: string, data: { name: string; oidc_issuer: string }) => 
    apiFetch<{tenant_id: string}>('POST', '/admin/tenants', data, token),
  deactivateTenant: (token: string, id: string) => 
    apiFetch<void>('POST', `/admin/tenants/${id}/deactivate`, undefined, token),
  updateOidcIssuer: (token: string, id: string, issuer: string) => 
    apiFetch<void>('PUT', `/admin/tenants/${id}/oidc-issuer`, { oidc_issuer: issuer }, token),
  getTenantUsage: (token: string, id: string) => 
    apiFetch<UsageMetrics>('GET', `/admin/tenants/${id}/usage`, undefined, token),
  
  listRegistrations: (token: string, status?: string) => 
    apiFetch<TenantRegistration[]>('GET', `/admin/registrations${status ? `?status=${status}` : ''}`, undefined, token),
  approveRegistration: (token: string, id: string) => 
    apiFetch<{tenant_id: string}>('POST', `/admin/registrations/${id}/approve`, undefined, token),
  rejectRegistration: (token: string, id: string, reason?: string) => 
    apiFetch<void>('POST', `/admin/registrations/${id}/reject`, reason ? { reason } : {}, token),

  // Public/Registration
  submitRegistration: (data: { app_name: string; oidc_issuer: string; contact_email: string }) => 
    apiFetch<SubmitRegistrationResponse>('POST', '/registrations', data),
  getRegistrationStatus: (id: string, token: string) => 
    apiFetch<TenantRegistration>('GET', `/registrations/${id}`, undefined, token),
};
