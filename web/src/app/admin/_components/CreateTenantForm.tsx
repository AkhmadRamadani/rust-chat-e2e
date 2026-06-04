'use client';
import React, { useState } from 'react';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useToast } from '@/components/ui/Toast';
import { api } from '@/lib/api';

export function CreateTenantForm({ token, onSuccess }: { token: string; onSuccess: () => void }) {
  const [name, setName] = useState('');
  const [issuer, setIssuer] = useState('');
  const [loading, setLoading] = useState(false);
  const { showToast } = useToast();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return showToast('Name is required', 'error');
    if (!issuer.startsWith('https://') && !issuer.startsWith('http://')) return showToast('Issuer must start with http:// or https://', 'error');

    setLoading(true);
    try {
      await api.createTenant(token, { name, oidc_issuer: issuer });
      showToast('Tenant created successfully', 'success');
      setName('');
      setIssuer('');
      onSuccess();
    } catch (err: any) {
      showToast(err.message || 'Failed to create tenant', 'error');
    } finally {
      setLoading(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} className="flex gap-4 items-end mb-6">
      <div className="flex-1">
        <Input 
          label="Tenant Name" 
          value={name} 
          onChange={(e) => setName(e.target.value)} 
          placeholder="e.g. Acme Corp" 
        />
      </div>
      <div className="flex-1">
        <Input 
          label="OIDC Issuer URL" 
          value={issuer} 
          onChange={(e) => setIssuer(e.target.value)} 
          placeholder="https://..." 
        />
      </div>
      <Button type="submit" disabled={loading}>Create Tenant</Button>
    </form>
  );
}
