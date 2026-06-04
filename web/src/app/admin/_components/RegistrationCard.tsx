'use client';
import React, { useState } from 'react';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { Modal } from '@/components/ui/Modal';
import { Input } from '@/components/ui/Input';
import { TenantRegistration } from '@/lib/types';
import { api } from '@/lib/api';
import { useToast } from '@/components/ui/Toast';

export function RegistrationCard({
  reg,
  token,
  onRefresh,
}: {
  reg: TenantRegistration;
  token: string;
  onRefresh: () => void;
}) {
  const { showToast } = useToast();
  const [rejectModalOpen, setRejectModalOpen] = useState(false);
  const [rejectReason, setRejectReason] = useState('');
  const [actionLoading, setActionLoading] = useState(false);

  const handleApprove = async () => {
    setActionLoading(true);
    try {
      await api.approveRegistration(token, reg.registration_id);
      showToast('Registration approved, tenant created', 'success');
      onRefresh();
    } catch (err: any) {
      showToast(err.message, 'error');
    } finally {
      setActionLoading(false);
    }
  };

  const handleReject = async () => {
    setActionLoading(true);
    try {
      await api.rejectRegistration(token, reg.registration_id, rejectReason);
      showToast('Registration rejected', 'success');
      setRejectModalOpen(false);
      onRefresh();
    } catch (err: any) {
      showToast(err.message, 'error');
    } finally {
      setActionLoading(false);
    }
  };

  return (
    <Card className="p-4 flex flex-col gap-2">
      <div className="flex justify-between items-start">
        <h4 className="font-semibold text-lg">{reg.app_name}</h4>
        <Badge variant={reg.status === 'pending' ? 'pending' : reg.status === 'approved' ? 'active' : 'inactive'}>
          {reg.status.charAt(0).toUpperCase() + reg.status.slice(1)}
        </Badge>
      </div>
      
      <div className="text-sm text-text-dim flex flex-col gap-1 mt-2">
        <div><span className="font-medium text-text">Email:</span> {reg.contact_email}</div>
        <div><span className="font-medium text-text">Issuer:</span> {reg.oidc_issuer}</div>
        <div><span className="font-medium text-text">Date:</span> {new Date(reg.created_at).toLocaleString()}</div>
        {reg.tenant_id && <div><span className="font-medium text-text">Tenant ID:</span> {reg.tenant_id}</div>}
        {reg.rejection_reason && <div><span className="font-medium text-danger">Reason:</span> {reg.rejection_reason}</div>}
      </div>

      {reg.status === 'pending' && (
        <div className="flex gap-2 mt-4">
          <Button size="sm" onClick={handleApprove} disabled={actionLoading}>Approve</Button>
          <Button size="sm" variant="danger-outline" onClick={() => setRejectModalOpen(true)} disabled={actionLoading}>Reject</Button>
        </div>
      )}

      <Modal
        isOpen={rejectModalOpen}
        onClose={() => setRejectModalOpen(false)}
        title="Reject Registration"
        actions={
          <>
            <Button variant="ghost" onClick={() => setRejectModalOpen(false)}>Cancel</Button>
            <Button variant="danger-outline" onClick={handleReject} disabled={actionLoading}>Confirm Reject</Button>
          </>
        }
      >
        <div className="space-y-4">
          <p>Provide an optional reason for rejecting <strong>{reg.app_name}</strong>.</p>
          <Input 
            label="Rejection Reason" 
            placeholder="e.g. Invalid issuer URL" 
            value={rejectReason}
            onChange={(e) => setRejectReason(e.target.value)}
          />
        </div>
      </Modal>
    </Card>
  );
}
