'use client';
import React, { useEffect, useState, useCallback } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useToast } from '@/components/ui/Toast';
import { Badge } from '@/components/ui/Badge';
import { api } from '@/lib/api';
import { auth } from '@/lib/auth';
import { Tenant, TenantRegistration } from '@/lib/types';
import { CreateTenantForm } from './_components/CreateTenantForm';
import { TenantCard } from './_components/TenantCard';
import { RegistrationCard } from './_components/RegistrationCard';
import { UsageModal } from './_components/UsageModal';

type AdminView = 'login' | 'dashboard';

export default function AdminPage() {
  const [view, setView] = useState<AdminView>('login');
  const [tokenInput, setTokenInput] = useState('');
  const [loading, setLoading] = useState(false);
  const { showToast } = useToast();

  const [tenants, setTenants] = useState<Tenant[]>([]);
  const [registrations, setRegistrations] = useState<TenantRegistration[]>([]);
  
  const [usageModalTarget, setUsageModalTarget] = useState<{id: string, name: string} | null>(null);

  const loadDashboardData = useCallback(async (token: string) => {
    try {
      const [tData, rData] = await Promise.all([
        api.listTenants(token),
        api.listRegistrations(token)
      ]);
      setTenants(tData);
      setRegistrations(rData);
    } catch (err: any) {
      if (err.status === 401 || err.status === 403) {
        auth.clearAdminToken();
        setView('login');
        showToast('Session expired or invalid token', 'error');
      } else {
        showToast('Failed to load dashboard data', 'error');
      }
    }
  }, [showToast]);

  useEffect(() => {
    const existingToken = auth.getAdminToken();
    if (existingToken) {
      setView('dashboard');
      loadDashboardData(existingToken);
    }
  }, [loadDashboardData]);

  const handleLogin = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!tokenInput.trim()) return;
    setLoading(true);
    try {
      await api.listTenants(tokenInput); // validate token
      auth.setAdminToken(tokenInput);
      setView('dashboard');
      loadDashboardData(tokenInput);
    } catch (err: any) {
      showToast(err.status === 401 ? 'Invalid token' : 'Server error', 'error');
    } finally {
      setLoading(false);
    }
  };

  const handleSignOut = () => {
    auth.clearAdminToken();
    setView('login');
    setTokenInput('');
  };

  if (view === 'login') {
    return (
      <div className="flex items-center justify-center min-h-screen p-4 bg-bg">
        <div className="w-full max-w-sm bg-surface border border-border rounded-xl p-8 shadow-2xl">
          <h1 className="text-2xl font-bold text-center mb-6">Admin Login</h1>
          <form onSubmit={handleLogin} className="flex flex-col gap-4">
            <Input 
              type="password" 
              label="Admin Token" 
              value={tokenInput} 
              onChange={e => setTokenInput(e.target.value)} 
              placeholder="Enter your admin token"
              autoFocus
            />
            <Button type="submit" disabled={loading} className="w-full mt-2">
              {loading ? 'Authenticating...' : 'Sign In'}
            </Button>
          </form>
        </div>
      </div>
    );
  }

  const token = auth.getAdminToken()!;
  const pendingRegs = registrations.filter(r => r.status === 'pending');
  const pastRegs = registrations.filter(r => r.status !== 'pending');

  return (
    <div className="min-h-screen bg-bg">
      <header className="bg-surface border-b border-border py-4 px-6 sticky top-0 z-10 flex justify-between items-center shadow-sm">
        <div className="flex items-center gap-4">
          <h1 className="text-xl font-bold text-accent">Tenant Admin Portal</h1>
          {pendingRegs.length > 0 && (
            <Badge variant="pending">{pendingRegs.length} Pending</Badge>
          )}
        </div>
        <Button variant="ghost" size="sm" onClick={handleSignOut}>Sign Out</Button>
      </header>

      <main className="max-w-7xl mx-auto p-6 flex flex-col gap-10">
        
        {/* Registrations Section */}
        <section>
          <h2 className="text-2xl font-semibold mb-6 flex items-center gap-3">
            Registrations
            <Badge variant="active" className="text-xs bg-surface2 text-text border-border">
              {registrations.length} Total
            </Badge>
          </h2>
          
          {pendingRegs.length > 0 && (
            <div className="mb-6">
              <h3 className="text-lg font-medium text-warning mb-3">Action Required</h3>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                {pendingRegs.map(r => (
                  <RegistrationCard key={r.registration_id} reg={r} token={token} onRefresh={() => loadDashboardData(token)} />
                ))}
              </div>
            </div>
          )}

          {pastRegs.length > 0 && (
            <div>
              <h3 className="text-lg font-medium text-text-dim mb-3">Past Registrations</h3>
              <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 opacity-75">
                {pastRegs.map(r => (
                  <RegistrationCard key={r.registration_id} reg={r} token={token} onRefresh={() => loadDashboardData(token)} />
                ))}
              </div>
            </div>
          )}

          {registrations.length === 0 && (
            <div className="text-center py-10 bg-surface rounded-lg border border-border border-dashed text-text-dim">
              No registrations found.
            </div>
          )}
        </section>

        <div className="h-px bg-border w-full"></div>

        {/* Tenants Section */}
        <section>
          <div className="flex justify-between items-end mb-6">
            <h2 className="text-2xl font-semibold">Active Tenants</h2>
          </div>
          
          <div className="bg-surface p-6 rounded-lg border border-border mb-8 shadow-sm">
            <h3 className="text-lg font-medium mb-4">Direct Provisioning</h3>
            <CreateTenantForm token={token} onSuccess={() => loadDashboardData(token)} />
          </div>

          {tenants.length > 0 ? (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {tenants.map(t => (
                <TenantCard 
                  key={t.tenant_id} 
                  tenant={t} 
                  token={token} 
                  onRefresh={() => loadDashboardData(token)}
                  onShowUsage={(id, name) => setUsageModalTarget({id, name})}
                />
              ))}
            </div>
          ) : (
            <div className="text-center py-10 bg-surface rounded-lg border border-border border-dashed text-text-dim">
              No tenants found.
            </div>
          )}
        </section>

      </main>

      <UsageModal 
        isOpen={usageModalTarget !== null}
        onClose={() => setUsageModalTarget(null)}
        token={token}
        tenantId={usageModalTarget?.id || ''}
        tenantName={usageModalTarget?.name || ''}
      />
    </div>
  );
}
