'use client';
import React, { useEffect, useState } from 'react';
import { Modal } from '@/components/ui/Modal';
import { api } from '@/lib/api';
import { UsageMetrics } from '@/lib/types';
import { useToast } from '@/components/ui/Toast';

export function UsageModal({
  token,
  tenantId,
  tenantName,
  isOpen,
  onClose,
}: {
  token: string;
  tenantId: string;
  tenantName: string;
  isOpen: boolean;
  onClose: () => void;
}) {
  const [metrics, setMetrics] = useState<UsageMetrics | null>(null);
  const [loading, setLoading] = useState(false);
  const { showToast } = useToast();

  useEffect(() => {
    if (isOpen && tenantId) {
      setLoading(true);
      api.getTenantUsage(token, tenantId)
        .then(setMetrics)
        .catch(err => showToast(err.message, 'error'))
        .finally(() => setLoading(false));
    } else {
      setMetrics(null);
    }
  }, [isOpen, tenantId, token, showToast]);

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={`Usage: ${tenantName}`}>
      {loading ? (
        <p className="text-text-dim">Loading metrics...</p>
      ) : metrics ? (
        <div className="space-y-4">
          <div className="flex justify-between border-b border-border pb-2">
            <span className="text-text-dim">Users</span>
            <span className="font-semibold">{metrics.user_count}</span>
          </div>
          <div className="flex justify-between border-b border-border pb-2">
            <span className="text-text-dim">Devices</span>
            <span className="font-semibold">{metrics.device_count}</span>
          </div>
          <div className="flex justify-between border-b border-border pb-2">
            <span className="text-text-dim">Messages (30d)</span>
            <span className="font-semibold">{metrics.message_count_30d}</span>
          </div>
          <div className="flex justify-between border-b border-border pb-2">
            <span className="text-text-dim">Active WebSocket Sessions</span>
            <span className="font-semibold">{metrics.active_wt_sessions}</span>
          </div>
        </div>
      ) : (
        <p className="text-danger">Failed to load metrics.</p>
      )}
    </Modal>
  );
}
