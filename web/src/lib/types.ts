export interface Tenant {
  tenant_id: string;
  name: string;
  oidc_issuer: string;
  active: boolean;
}

export interface TenantRegistration {
  registration_id: string;
  app_name: string;
  contact_email: string;
  oidc_issuer: string;
  status: 'pending' | 'approved' | 'rejected';
  created_at: string;
  tenant_id: string | null;
  rejection_reason: string | null;
}

export interface SubmitRegistrationResponse {
  registration_id: string;
  registration_token: string;
}

export interface UsageMetrics {
  user_count: number;
  device_count: number;
  message_count_30d: number;
  active_wt_sessions: number;
}

export class ApiError extends Error {
  error_code: string;
  request_id?: string;
  status: number;

  constructor(status: number, error_code: string, message: string, request_id?: string) {
    super(message);
    this.status = status;
    this.error_code = error_code;
    this.request_id = request_id;
    this.name = 'ApiError';
  }
}
