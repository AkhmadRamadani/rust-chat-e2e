'use client';

import React, { useState } from 'react';
import { useDevClientStore, TestUser } from './useDevClientStore';
import { useDevLog, DevLog } from './DevLog';
import { apiFetch } from './apiHelpers';
import { buildBundle, dummyKey32 } from './crypto';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useToast } from '@/components/ui/Toast';

export function UsersTab({ onSwitchToChat }: { onSwitchToChat: () => void }) {
  const store = useDevClientStore();
  const { logs, appendLog, clearLog } = useDevLog();
  const toast = useToast();

  const [tokenUserId, setTokenUserId] = useState('');
  const [tokenIssuer, setTokenIssuer] = useState('http://oidc/default');
  const [tokenValue, setTokenValue] = useState('');

  const [newUserId, setNewUserId] = useState('');
  const [newUserToken, setNewUserToken] = useState('');
  const [newUserOtpks, setNewUserOtpks] = useState('10');

  const [replenishUserId, setReplenishUserId] = useState('');
  const [replenishDeviceId, setReplenishDeviceId] = useState('');
  const [replenishToken, setReplenishToken] = useState('');
  const [replenishCount, setReplenishCount] = useState('20');

  const handleFetchToken = async () => {
    if (!tokenUserId || !tokenIssuer) {
      toast.showToast('Enter a user ID and OIDC issuer.', 'error');
      return;
    }
    
    let mockPath = 'default';
    if (tokenIssuer.startsWith('http://oidc/')) {
      mockPath = tokenIssuer.replace('http://oidc/', '');
    } else if (tokenIssuer.startsWith('http://oidc:80/')) {
      mockPath = tokenIssuer.replace('http://oidc:80/', '');
    } else {
      toast.showToast('Issuer must start with http://oidc/ for dev mock server', 'error');
      return;
    }

    appendLog('info', `Issuing token for ${tokenUserId} at ${tokenIssuer}…`);
    try {
      const res = await fetch(`/oidc/${encodeURIComponent(mockPath)}/token`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: `grant_type=client_credentials&client_id=${encodeURIComponent(tokenUserId)}&client_secret=any`,
      });
      const data = await res.json();
      if (data.access_token) {
        setTokenValue(data.access_token);
        setNewUserId(tokenUserId);
        setNewUserToken(data.access_token);
        appendLog('ok', `Token issued for ${tokenUserId} (iss=${tokenIssuer})`);
        toast.showToast(`Token issued for ${tokenUserId}`, 'success');
      } else {
        appendLog('err', JSON.stringify(data));
        toast.showToast('Failed to issue token', 'error');
      }
    } catch (e: any) {
      appendLog('err', `Token fetch error: ${e.message}`);
      toast.showToast('OIDC unreachable — is the oidc container running?', 'error');
    }
  };

  const copyTokenValue = () => {
    if (tokenValue) {
      navigator.clipboard.writeText(tokenValue).then(() => toast.showToast('Token copied!', 'success'));
    }
  };

  const handleRegisterNewUser = async () => {
    if (!newUserId) { toast.showToast('Enter a User ID.', 'error'); return; }
    if (!newUserToken) { toast.showToast('Enter a Bearer Token for this user.', 'error'); return; }

    const otpkCount = parseInt(newUserOtpks) || 10;
    const bundle = await buildBundle(Math.min(100, Math.max(1, otpkCount)));
    
    const res = await apiFetch('POST', `/users/${encodeURIComponent(newUserId)}/devices`, bundle, newUserToken, appendLog);

    if (res.ok && res.data?.device_id) {
      const deviceId = res.data.device_id;
      const existing = store.users.findIndex(u => u.userId === newUserId);
      const entry = { userId: newUserId, deviceId, token: newUserToken };
      
      const newUsers = [...store.users];
      if (existing >= 0) newUsers[existing] = entry;
      else newUsers.push(entry);
      
      store.update({ users: newUsers });
      toast.showToast(`User "${newUserId}" registered — device ${deviceId.slice(0, 8)}…`, 'success');
      
      setNewUserId('');
      setNewUserToken('');
      setNewUserOtpks('10');
    }
  };

  const handleReplenishKeys = async () => {
    if (!replenishUserId || !replenishDeviceId || !replenishToken) {
      toast.showToast('Fill in all replenishment fields.', 'error');
      return;
    }
    const count = parseInt(replenishCount) || 20;
    const keys = Array.from({ length: Math.min(100, count) }, (_, i) => ({
      key_id: Date.now() + i,
      public_key: dummyKey32(),
    }));

    const res = await apiFetch(
      'PUT',
      `/users/${encodeURIComponent(replenishUserId)}/devices/${encodeURIComponent(replenishDeviceId)}/one-time-prekeys`,
      { one_time_prekeys: keys },
      replenishToken,
      appendLog
    );
    if (res.ok) {
      toast.showToast(`Replenished ${count} OTPKs. Total: ${res.data?.total_count}`, 'success');
    }
  };

  const useUser = (u: TestUser) => {
    store.update({
      myUserId: u.userId,
      myDeviceId: u.deviceId,
      token: u.token,
    });
    onSwitchToChat();
    toast.showToast(`Switched to ${u.userId}`, 'success');
  };

  const removeUser = (userId: string) => {
    store.update({ users: store.users.filter((u) => u.userId !== userId) });
  };

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      <aside className="w-[300px] shrink-0 flex flex-col bg-[#1a1d27] border-r border-[#2e3250] overflow-y-auto">
        {/* Get Token */}
        <div className="p-4 border-b border-[#2e3250]">
          <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] mb-3">Get Token (Mock OIDC)</div>
          <p className="text-[12px] text-[#7b82a8] mb-3 leading-relaxed">
            Issues a JWT from the mock OIDC server. Tenant issuer must start with <code className="text-[#a5aff9] text-[11px]">http://oidc/</code>
          </p>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">OIDC Issuer</label>
            <Input
              list="tenant-issuers"
              placeholder="http://oidc/my-tenant"
              value={tokenIssuer}
              onChange={(e) => setTokenIssuer(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
            <datalist id="tenant-issuers">
              <option value="http://oidc/default" />
              {store.tenants
                .filter(t => t.oidc_issuer.startsWith('http://oidc/'))
                .filter(t => t.oidc_issuer !== 'http://oidc/default')
                .map(t => (
                  <option key={t.tenant_id} value={t.oidc_issuer} />
                ))}
            </datalist>
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">User ID (becomes JWT sub)</label>
            <Input
              placeholder="alice"
              value={tokenUserId}
              onChange={(e) => setTokenUserId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <Button onClick={handleFetchToken} className="w-full bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white h-8 text-[12px]">Issue Token</Button>
          
          {tokenValue && (
            <div className="mt-2.5">
              <label className="text-[11px] text-[#7b82a8]">Token (click to copy)</label>
              <textarea
                readOnly
                rows={3}
                value={tokenValue}
                onClick={copyTokenValue}
                className="w-full bg-[#22263a] border border-[#2e3250] rounded-md text-[#e2e4f0] text-[10px] font-mono p-2 mt-1 cursor-pointer outline-none focus:border-[#5c6ef8]"
              />
            </div>
          )}
        </div>

        {/* Register User */}
        <div className="p-4 border-b border-[#2e3250]">
          <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] mb-3">Register New User</div>
          <p className="text-[12px] text-[#7b82a8] mb-3 leading-relaxed">
            Users are implicitly created when their first device is registered. Paste a valid JWT for that user.
          </p>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">User ID (JWT sub claim)</label>
            <Input
              placeholder="user-charlie"
              value={newUserId}
              onChange={(e) => setNewUserId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Bearer Token for this user</label>
            <Input
              placeholder="eyJ…"
              value={newUserToken}
              onChange={(e) => setNewUserToken(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Number of OTPKs (1–100)</label>
            <Input
              type="number"
              value={newUserOtpks}
              onChange={(e) => setNewUserOtpks(e.target.value)}
              min="1" max="100"
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <Button onClick={handleRegisterNewUser} className="w-full bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white h-8 text-[12px]">Register & Create User</Button>
        </div>

        {/* Replenish Keys */}
        <div className="p-4 border-b border-[#2e3250]">
          <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] mb-3">Replenish Keys</div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">User ID</label>
            <Input
              placeholder="user-alice"
              value={replenishUserId}
              onChange={(e) => setReplenishUserId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Device ID (UUID)</label>
            <Input
              placeholder="00000000-…"
              value={replenishDeviceId}
              onChange={(e) => setReplenishDeviceId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Bearer Token</label>
            <Input
              placeholder="eyJ…"
              value={replenishToken}
              onChange={(e) => setReplenishToken(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Keys to add (1–100)</label>
            <Input
              type="number"
              value={replenishCount}
              onChange={(e) => setReplenishCount(e.target.value)}
              min="1" max="100"
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <Button variant="ghost" onClick={handleReplenishKeys} className="w-full h-8 text-[12px] bg-transparent border-[#2e3250] text-[#7b82a8] hover:border-[#5c6ef8] hover:text-[#e2e4f0]">Replenish OTPKs</Button>
        </div>
      </aside>

      <div className="flex-1 flex flex-col overflow-hidden bg-[#0f1117]">
        <div className="p-4 px-5 border-b border-[#2e3250] flex items-center shrink-0">
          <span className="text-[15px] font-semibold text-[#e2e4f0]">Registered Users</span>
        </div>
        <div className="flex-1 overflow-y-auto p-4">
          {store.users.length === 0 ? (
            <div className="text-[#7b82a8] text-[13px]">Register a user using the form on the left. Registered users are stored locally in this browser session.</div>
          ) : (
            <div className="flex flex-col gap-2">
              {store.users.map((u) => (
                <div key={u.userId} className="bg-[#22263a] border border-[#2e3250] rounded-lg p-2.5">
                  <div className="flex items-center gap-2.5 mb-1">
                    <div className="w-8 h-8 rounded-full bg-[#3d4ab0] flex items-center justify-center text-[12px] font-bold text-[#e2e4f0] shrink-0">
                      {u.userId.slice(0, 2).toUpperCase()}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="text-[13px] font-semibold text-[#e2e4f0] truncate">{u.userId}</div>
                      <div className="text-[11px] text-[#7b82a8] font-mono truncate mt-0.5">Device: {u.deviceId}</div>
                    </div>
                  </div>
                  <div className="flex gap-1.5 mt-2">
                    <Button
                      size="sm"
                      onClick={() => useUser(u)}
                      className="bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white h-7 text-[10px] px-2"
                    >
                      Use in Chat
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        navigator.clipboard.writeText(u.deviceId);
                        toast.showToast('Device ID copied', 'success');
                      }}
                      className="h-7 text-[10px] px-2 bg-transparent border-[#2e3250] text-[#7b82a8] hover:text-[#e2e4f0] hover:border-[#5c6ef8]"
                    >
                      Copy Device ID
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => removeUser(u.userId)}
                      className="h-7 text-[10px] px-2 bg-transparent border-[#f87171] text-[#f87171] hover:bg-[#f87171] hover:bg-opacity-10"
                    >
                      Remove
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      <DevLog logs={logs} onClear={clearLog} />
    </div>
  );
}
