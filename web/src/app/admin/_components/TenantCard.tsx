'use client';
import React, { useState } from 'react';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { CopyButton } from '@/components/ui/CopyButton';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { Tenant } from '@/lib/types';
import { api } from '@/lib/api';
import { useToast } from '@/components/ui/Toast';

export function TenantCard({
  tenant,
  token,
  onRefresh,
  onShowUsage,
}: {
  tenant: Tenant;
  token: string;
  onRefresh: () => void;
  onShowUsage: (id: string, name: string) => void;
}) {
  const { showToast } = useToast();
  const [editing, setEditing] = useState(false);
  const [issuer, setIssuer] = useState(tenant.oidc_issuer);
  const [saving, setSaving] = useState(false);
  const [deactivateModalOpen, setDeactivateModalOpen] = useState(false);

  const handleUpdateIssuer = async () => {
    if (!issuer.startsWith('https://')) return showToast('Must be https', 'error');
    setSaving(true);
    try {
      await api.updateOidcIssuer(token, tenant.tenant_id, issuer);
      showToast('Issuer updated', 'success');
      setEditing(false);
      onRefresh();
    } catch (err: any) {
      showToast(err.message, 'error');
    } finally {
      setSaving(false);
    }
  };

  const handleDeactivate = async () => {
    try {
      await api.deactivateTenant(token, tenant.tenant_id);
      showToast('Tenant deactivated', 'success');
      setDeactivateModalOpen(false);
      onRefresh();
    } catch (err: any) {
      showToast(err.message, 'error');
    }
  };

  return (
    <Card className="p-4 flex flex-col gap-3">
      <div className="flex justify-between items-start">
        <div>
          <h4 className="font-semibold text-lg">{tenant.name}</h4>
          <div className="flex items-center gap-2 mt-1">
            <span className="text-xs text-text-dim font-mono">{tenant.tenant_id}</span>
            <CopyButton text={tenant.tenant_id} />
          </div>
        </div>
        <Badge variant={tenant.active ? 'active' : 'inactive'}>
          {tenant.active ? 'Active' : 'Inactive'}
        </Badge>
      </div>

      <div className="bg-surface p-3 rounded border border-border">
        <div className="text-sm font-medium text-text-dim mb-1">OIDC Issuer</div>
        {editing ? (
          <div className="flex gap-2">
            <Input value={issuer} onChange={(e) => setIssuer(e.target.value)} />
            <Button size="sm" onClick={handleUpdateIssuer} disabled={saving}>Save</Button>
            <Button size="sm" variant="ghost" onClick={() => { setEditing(false); setIssuer(tenant.oidc_issuer); }}>Cancel</Button>
          </div>
        ) : (
          <div className="flex justify-between items-center">
            <span className="text-sm">{tenant.oidc_issuer}</span>
            {tenant.active && <Button size="sm" variant="ghost" onClick={() => setEditing(true)}>Edit</Button>}
          </div>
        )}
      </div>

      <div className="flex gap-2 mt-2">
        <Button size="sm" variant="ghost" onClick={() => onShowUsage(tenant.tenant_id, tenant.name)}>
          View Usage
        </Button>
        {tenant.active && (
          <Button size="sm" variant="danger-outline" onClick={() => setDeactivateModalOpen(true)}>
            Deactivate
          </Button>
        )}
      </div>

      <Modal
        isOpen={deactivateModalOpen}
        onClose={() => setDeactivateModalOpen(false)}
        title="Deactivate Tenant"
        actions={
          <>
            <Button variant="ghost" onClick={() => setDeactivateModalOpen(false)}>Cancel</Button>
            <Button variant="danger-outline" onClick={handleDeactivate}>Deactivate</Button>
          </>
        }
      >
        <p>Are you sure you want to deactivate <strong>{tenant.name}</strong>? This action cannot be undone.</p>
      </Modal>
    </Card>
  );
}
