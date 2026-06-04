'use client';
import React, { useEffect, useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useToast } from '@/components/ui/Toast';
import { Card } from '@/components/ui/Card';
import { CopyButton } from '@/components/ui/CopyButton';
import { Badge } from '@/components/ui/Badge';
import { api } from '@/lib/api';
import { auth } from '@/lib/auth';
import { TenantRegistration } from '@/lib/types';

type RegisterView = 'form' | 'success' | 'status';

export default function RegisterPage() {
  const [view, setView] = useState<RegisterView>('form');
  const { showToast } = useToast();

  // Form state
  const [appName, setAppName] = useState('');
  const [oidcIssuer, setOidcIssuer] = useState('');
  const [contactEmail, setContactEmail] = useState('');
  const [formLoading, setFormLoading] = useState(false);
  const [formErrors, setFormErrors] = useState<Record<string, string>>({});

  // Success state
  const [regId, setRegId] = useState('');
  const [regToken, setRegToken] = useState('');

  // Status state
  const [statusLoading, setStatusLoading] = useState(false);
  const [registration, setRegistration] = useState<TenantRegistration | null>(null);

  useEffect(() => {
    const creds = auth.getRegistrationCredentials();
    if (creds) {
      setRegId(creds.id);
      setRegToken(creds.token);
      setView('status');
      checkStatus(creds.id, creds.token);
    }
  }, []);

  const validateForm = () => {
    const errors: Record<string, string> = {};
    if (!appName.trim() || appName.length > 100) errors.appName = 'Must be between 1 and 100 characters';
    if (!oidcIssuer.startsWith('https://')) errors.oidcIssuer = 'Must start with https://';
    if (!/^[^@]+@[^@]+\.[^@]+$/.test(contactEmail)) errors.contactEmail = 'Must be a valid email address';
    setFormErrors(errors);
    return Object.keys(errors).length === 0;
  };

  const handleRegisterSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!validateForm()) return;

    setFormLoading(true);
    try {
      const res = await api.submitRegistration({
        app_name: appName.trim(),
        oidc_issuer: oidcIssuer.trim(),
        contact_email: contactEmail.trim()
      });
      auth.setRegistrationCredentials(res.registration_id, res.registration_token);
      setRegId(res.registration_id);
      setRegToken(res.registration_token);
      showToast('Registration submitted successfully', 'success');
      setView('success');
    } catch (err: any) {
      if (err.error_code === 'issuer_already_registered') {
        setFormErrors({ oidcIssuer: 'This issuer is already registered or pending.' });
      } else {
        showToast(err.message, 'error');
      }
    } finally {
      setFormLoading(false);
    }
  };

  const checkStatus = async (id: string, token: string) => {
    setStatusLoading(true);
    try {
      const reg = await api.getRegistrationStatus(id, token);
      setRegistration(reg);
    } catch (err: any) {
      if (err.status === 401) {
        showToast('Invalid registration token', 'error');
      } else if (err.status === 404) {
        showToast('Registration not found', 'error');
      } else {
        showToast(err.message, 'error');
      }
    } finally {
      setStatusLoading(false);
    }
  };

  const handleStatusSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!regId || !regToken) return;
    auth.setRegistrationCredentials(regId, regToken);
    checkStatus(regId, regToken);
  };

  const resetForm = () => {
    auth.clearRegistrationCredentials();
    setAppName('');
    setOidcIssuer('');
    setContactEmail('');
    setRegistration(null);
    setView('form');
  };

  return (
    <div className="min-h-screen bg-bg py-12 px-4 sm:px-6 lg:px-8 flex flex-col items-center">
      <div className="w-full max-w-md text-center mb-8">
        <h1 className="text-3xl font-bold text-accent mb-2">Tenant Registration</h1>
        <p className="text-text-dim">Apply for access to the platform</p>
      </div>

      <div className="w-full max-w-xl">
        {view === 'form' && (
          <Card className="p-8 shadow-xl">
            <form onSubmit={handleRegisterSubmit} className="flex flex-col gap-5">
              <Input
                label="Application Name"
                placeholder="e.g. Acme Chat Client"
                value={appName}
                onChange={e => setAppName(e.target.value)}
                error={formErrors.appName}
              />
              <Input
                label="OIDC Issuer URL"
                placeholder="https://auth.acme.com"
                value={oidcIssuer}
                onChange={e => setOidcIssuer(e.target.value)}
                error={formErrors.oidcIssuer}
              />
              <Input
                label="Contact Email"
                type="email"
                placeholder="admin@acme.com"
                value={contactEmail}
                onChange={e => setContactEmail(e.target.value)}
                error={formErrors.contactEmail}
              />
              <div className="pt-2">
                <Button type="submit" className="w-full" disabled={formLoading}>
                  {formLoading ? 'Submitting...' : 'Submit Application'}
                </Button>
              </div>
            </form>
            <div className="mt-6 text-center">
              <button 
                onClick={() => setView('status')} 
                className="text-sm text-text-dim hover:text-accent transition-colors"
              >
                Already registered? Check Status &rarr;
              </button>
            </div>
          </Card>
        )}

        {view === 'success' && (
          <Card className="p-8 shadow-xl text-center">
            <div className="w-16 h-16 bg-success/20 text-success rounded-full flex items-center justify-center mx-auto mb-6">
              <svg className="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" d="M5 13l4 4L19 7"></path></svg>
            </div>
            <h2 className="text-2xl font-bold mb-2">Application Submitted!</h2>
            <p className="text-text-dim mb-8">Your registration is pending administrator approval.</p>

            <div className="bg-surface border border-warning/50 rounded-lg p-5 mb-8 text-left">
              <div className="flex items-center gap-2 text-warning font-semibold mb-3">
                <span>⚠️</span>
                IMPORTANT: Save your token
              </div>
              <p className="text-sm text-text-dim mb-4">
                You need this token to check your status and retrieve your Tenant ID later. It will not be shown again.
              </p>
              
              <div className="space-y-4">
                <div>
                  <div className="text-xs text-text-dim mb-1">Registration ID</div>
                  <div className="flex items-center justify-between bg-bg p-2 border border-border rounded font-mono text-sm">
                    {regId}
                    <CopyButton text={regId} />
                  </div>
                </div>
                <div>
                  <div className="text-xs text-text-dim mb-1">Registration Token</div>
                  <div className="flex items-center justify-between bg-bg p-2 border border-border rounded font-mono text-sm break-all">
                    <span className="truncate mr-2">{regToken}</span>
                    <CopyButton text={regToken} />
                  </div>
                </div>
              </div>
            </div>

            <Button onClick={() => setView('status')} className="w-full">
              Check Status &rarr;
            </Button>
          </Card>
        )}

        {view === 'status' && (
          <Card className="p-8 shadow-xl">
            <div className="flex justify-between items-center mb-6">
              <h2 className="text-xl font-bold">Registration Status</h2>
              <button onClick={resetForm} className="text-sm text-text-dim hover:text-text">
                Start Over
              </button>
            </div>

            <form onSubmit={handleStatusSubmit} className="flex flex-col gap-4 mb-8 pb-8 border-b border-border">
              <Input
                label="Registration ID"
                value={regId}
                onChange={e => setRegId(e.target.value)}
                placeholder="UUID"
                required
              />
              <Input
                label="Registration Token"
                type="password"
                value={regToken}
                onChange={e => setRegToken(e.target.value)}
                placeholder="64-character hex token"
                required
              />
              <Button type="submit" variant="ghost" className="w-full bg-surface2 border border-border" disabled={statusLoading}>
                {statusLoading ? 'Checking...' : 'Check Status'}
              </Button>
            </form>

            {registration && (
              <div className="bg-surface p-6 rounded-lg border border-border">
                <div className="flex justify-between items-start mb-4">
                  <h3 className="font-semibold text-lg">{registration.app_name}</h3>
                  <Badge variant={registration.status === 'pending' ? 'pending' : registration.status === 'approved' ? 'active' : 'inactive'}>
                    {registration.status.toUpperCase()}
                  </Badge>
                </div>
                
                <div className="space-y-2 text-sm">
                  <div className="flex justify-between">
                    <span className="text-text-dim">Issuer</span>
                    <span>{registration.oidc_issuer}</span>
                  </div>
                  <div className="flex justify-between">
                    <span className="text-text-dim">Submitted</span>
                    <span>{new Date(registration.created_at).toLocaleDateString()}</span>
                  </div>
                  
                  {registration.status === 'rejected' && registration.rejection_reason && (
                    <div className="mt-4 pt-4 border-t border-border">
                      <div className="text-danger font-medium text-xs uppercase mb-1">Rejection Reason</div>
                      <p className="text-text">{registration.rejection_reason}</p>
                    </div>
                  )}

                  {registration.status === 'approved' && registration.tenant_id && (
                    <div className="mt-4 pt-4 border-t border-border">
                      <div className="text-success font-medium text-xs uppercase mb-2">Provisioned Tenant ID</div>
                      <div className="flex items-center justify-between bg-bg p-2 border border-success/30 rounded font-mono text-sm text-success">
                        {registration.tenant_id}
                        <CopyButton text={registration.tenant_id} />
                      </div>
                      <p className="text-xs text-text-dim mt-2">
                        Use this Tenant ID along with your OIDC Issuer to configure your client application.
                      </p>
                    </div>
                  )}
                </div>
              </div>
            )}
          </Card>
        )}
      </div>
    </div>
  );
}
