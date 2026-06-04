'use client';

import { useState, useEffect } from 'react';

export interface TestUser {
  userId: string;
  deviceId: string;
  token: string;
}

export interface TestTenant {
  tenant_id: string;
  name: string;
  oidc_issuer: string;
  active: boolean;
}

export interface TestMessage {
  sender_user_id: string | { '0': string };
  seq?: number;
  server_ts?: string;
  _text: string;
  _mine: boolean;
  attachment_id?: string | null;
  attachment_name?: string | null;
  attachment_type?: string | null;
  attachment_size?: number | null;
}

export interface TestConversation {
  id: string;
  recipientUserId: string;
  recipientDeviceId?: string;
  messages: TestMessage[];
}

export interface DevClientState {
  adminUrl: string;
  token: string;
  myUserId: string;
  myDeviceId: string;
  adminToken: string;
  users: TestUser[];
  tenants: TestTenant[];
  conversations: Record<string, TestConversation>;
  activeConvId: string | null;
}

const defaultState: DevClientState = {
  adminUrl: '',
  token: '',
  myUserId: '',
  myDeviceId: '',
  adminToken: '',
  users: [],
  tenants: [],
  conversations: {},
  activeConvId: null,
};

// Simple global event target for state changes
class Store extends EventTarget {
  state: DevClientState = { ...defaultState };

  constructor() {
    super();
    if (typeof window !== 'undefined') {
      this.load();
    }
  }

  load() {
    try {
      const cfg = JSON.parse(localStorage.getItem('rustchat-cfg') || '{}');
      const st = JSON.parse(localStorage.getItem('rustchat-state') || '{}');
      const at = localStorage.getItem('adminToken') || '';

      this.state = {
        ...defaultState,
        adminUrl: cfg.adminUrl || '',
        token: cfg.token || '',
        myUserId: cfg.userId || '',
        myDeviceId: cfg.deviceId || '',
        adminToken: at,
        users: Array.isArray(st.users) ? st.users : [],
        tenants: Array.isArray(st.tenants) ? st.tenants : [],
        conversations: st.conversations || {},
        activeConvId: st.activeConvId || null,
      };
      this.dispatchEvent(new Event('change'));
    } catch (e) {
      console.error('Failed to load store', e);
    }
  }

  save() {
    if (typeof window === 'undefined') return;
    localStorage.setItem(
      'rustchat-cfg',
      JSON.stringify({
        adminUrl: this.state.adminUrl,
        token: this.state.token,
        userId: this.state.myUserId,
        deviceId: this.state.myDeviceId,
      })
    );
    localStorage.setItem('adminToken', this.state.adminToken);
    localStorage.setItem(
      'rustchat-state',
      JSON.stringify({
        users: this.state.users,
        tenants: this.state.tenants,
        conversations: this.state.conversations,
        activeConvId: this.state.activeConvId,
      })
    );
  }

  update(partial: Partial<DevClientState>) {
    this.state = { ...this.state, ...partial };
    this.save();
    this.dispatchEvent(new Event('change'));
  }
}

export const devStore = new Store();

export function useDevClientStore() {
  const [state, setState] = useState<DevClientState>(devStore.state);

  useEffect(() => {
    const handler = () => setState(devStore.state);
    devStore.addEventListener('change', handler);
    return () => devStore.removeEventListener('change', handler);
  }, []);

  return {
    ...state,
    update: (partial: Partial<DevClientState>) => devStore.update(partial),
  };
}
