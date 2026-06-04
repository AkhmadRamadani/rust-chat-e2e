'use client';

import React, { useState, useEffect, useRef, ChangeEvent } from 'react';
import { useDevClientStore, TestConversation, TestMessage, devStore } from './useDevClientStore';
import { useDevLog, DevLog } from './DevLog';
import { apiFetch } from './apiHelpers';
import { buildBundle, dummyKey32, encodeMsg, decodeMsg } from './crypto';
import { Button } from '@/components/ui/Button';
import { Input } from '@/components/ui/Input';
import { useToast } from '@/components/ui/Toast';

export function ChatTab({ setGlobalStatus }: { setGlobalStatus: (s: { ok: boolean, label: string }) => void }) {
  const store = useDevClientStore();
  const { logs, appendLog, clearLog } = useDevLog();
  const toast = useToast();

  const [recipientUserId, setRecipientUserId] = useState('');
  const [recipientDeviceId, setRecipientDeviceId] = useState('');
  const [msgInput, setMsgInput] = useState('');

  const wsRef = useRef<WebSocket | null>(null);
  const msgListRef = useRef<HTMLDivElement>(null);

  // Auto-scroll messages
  useEffect(() => {
    if (msgListRef.current) {
      msgListRef.current.scrollTop = msgListRef.current.scrollHeight;
    }
  }, [store.conversations, store.activeConvId]);

  // Auto-connect WS on mount
  useEffect(() => {
    connectWs(true);
    // Cleanup on unmount
    return () => {
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
    };
  }, []); // Run only once on mount

  const handleCheckHealth = async () => {
    const adminBase = store.adminUrl.replace(/\/$/, '') || 'http://localhost:3000';
    try {
      const res = await fetch(`${adminBase}/health`);
      const data = await res.json();
      const ok = res.ok && data.kds_storage === 'ok' && data.message_queue === 'ok';
      setGlobalStatus({ ok, label: ok ? 'connected' : 'degraded' });
      appendLog(ok ? 'ok' : 'warn', `Health: ${JSON.stringify(data)}`);
    } catch (e: any) {
      setGlobalStatus({ ok: false, label: 'unreachable' });
      appendLog('warn', `Health: ${e.message}`);
    }
  };

  const handleRegisterDevice = async () => {
    if (!store.myUserId) { toast.showToast('Set My User ID first.', 'error'); return; }
    const bundle = await buildBundle();
    const res = await apiFetch('POST', `/users/${encodeURIComponent(store.myUserId)}/devices`, bundle, store.token, appendLog);
    if (res.ok && res.data?.device_id) {
      store.update({ myDeviceId: res.data.device_id });
      toast.showToast(`Device registered: ${res.data.device_id}`, 'success');
    }
  };

  const connectWs = (auto = false) => {
    if (!store.token) {
      if (!auto) toast.showToast('Set your bearer token first.', 'error');
      return;
    }
    if (!store.myDeviceId) {
      if (!auto) toast.showToast('Register your device first.', 'error');
      return;
    }
    if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
      if (!auto) toast.showToast('Already connected', 'success');
      return;
    }

    const host = window.location.host;
    const deviceIdParam = store.myDeviceId ? `&device_id=${encodeURIComponent(store.myDeviceId)}` : '';
    const wsUrl = `ws://${host}/ws?token=${encodeURIComponent(store.token)}${deviceIdParam}`;
    appendLog('info', `Connecting WebSocket: ${wsUrl}`);
    const ws = new WebSocket(wsUrl);
    wsRef.current = ws;

    ws.onopen = () => {
      appendLog('ok', 'WebSocket connected');
      setGlobalStatus({ ok: true, label: 'WS connected' });
    };

    ws.onclose = (e) => {
      appendLog('warn', `WebSocket closed (${e.code})`);
      setGlobalStatus({ ok: false, label: 'WS disconnected' });
      wsRef.current = null;
    };

    ws.onerror = () => {
      appendLog('err', 'WebSocket error');
    };

    ws.onmessage = (e) => {
      try {
        const event = JSON.parse(e.data);
        if (event.type === 'ping') {
          ws.send(JSON.stringify({ type: 'pong' }));
          return;
        }
        handleRtEvent(event);
      } catch { }
    };
  };

  const handleRtEvent = (event: any) => {
    appendLog('info', `RT event: ${event.event || event.type}`);
    if (event.event === 'Message') {
      const m = event;
      const convId = typeof m.conversation_id === 'string' ? m.conversation_id : (m.conversation_id?.['0'] || null);
      if (!convId) return;

      const convs = { ...devStore.state.conversations };
      if (!convs[convId]) {
        const senderStr = typeof m.sender_user_id === 'string' ? m.sender_user_id : (m.sender_user_id?.['0'] || '?');
        const senderDevice = typeof m.sender_device_id === 'string' ? m.sender_device_id : (m.sender_device_id?.['0'] || undefined);
        convs[convId] = { id: convId, recipientUserId: senderStr, recipientDeviceId: senderDevice, messages: [] };
      }

      const conv = convs[convId];
      const existing = new Set(conv.messages.map(x => x.seq));

      if (!existing.has(m.seq)) {
        const senderStr = typeof m.sender_user_id === 'string' ? m.sender_user_id : (m.sender_user_id?.['0'] || '');
        const newMsg: TestMessage = {
          ...m,
          _text: decodeMsg(m.ciphertext),
          _mine: senderStr === devStore.state.myUserId
        };

        conv.messages = [...conv.messages, newMsg].sort((a, b) => (a.seq || 0) - (b.seq || 0));
        devStore.update({ conversations: convs });
      }
    }
  };

  const handleStartConversation = async () => {
    if (!store.myUserId || !store.myDeviceId) { toast.showToast('Register your device first.', 'error'); return; }
    if (!recipientUserId || !recipientDeviceId) { toast.showToast('Fill in recipient info.', 'error'); return; }

    const envelope = {
      conversation_id: '00000000-0000-0000-0000-000000000000',
      sender_user_id: store.myUserId,
      sender_device_id: store.myDeviceId,
      recipient_user_id: recipientUserId,
      recipient_device_id: recipientDeviceId,
      ciphertext: encodeMsg('__x3dh_init__'),
      protocol_header: {
        type: 'X3dhInit',
        sender_identity_key: dummyKey32(),
        ephemeral_key: dummyKey32(),
        used_signed_prekey_id: 1,
        used_otpk_id: 1,
        dr_header: { ratchet_key: dummyKey32(), prev_chain_n: 0, msg_n: 0 },
      },
    };

    const res = await apiFetch('POST', '/conversations', {
      recipient_user_id: recipientUserId,
      recipient_device_id: recipientDeviceId,
      envelope
    }, store.token, appendLog);

    if (res.ok && res.data?.conversation_id) {
      const convId = res.data.conversation_id;
      const convs = { ...store.conversations };
      if (!convs[convId]) {
        convs[convId] = { id: convId, recipientUserId, recipientDeviceId, messages: [] };
      }
      store.update({ conversations: convs, activeConvId: convId });
      toast.showToast('Conversation ready', 'success');
    }
  };

  const handleSendMessage = async () => {
    if (!msgInput || !store.activeConvId) return;
    const convId = store.activeConvId;
    const conv = store.conversations[convId];
    if (!conv) return;

    const text = msgInput;
    setMsgInput('');

    const envelope = {
      conversation_id: convId,
      sender_user_id: store.myUserId,
      sender_device_id: store.myDeviceId,
      recipient_user_id: conv.recipientUserId,
      recipient_device_id: conv.recipientDeviceId || null,
      ciphertext: encodeMsg(text),
      protocol_header: {
        type: 'DoubleRatchet', ratchet_key: dummyKey32(), prev_chain_n: 0, msg_n: conv.messages.length,
      },
      attachment_id: null,
    };

    const res = await apiFetch('POST', `/conversations/${convId}/messages`, { envelope }, store.token, appendLog);
    if (res.ok) {
      const newMsg: TestMessage = {
        sender_user_id: store.myUserId,
        seq: res.data?.seq,
        server_ts: res.data?.server_ts,
        _text: text,
        _mine: true,
      };

      const convs = { ...store.conversations };
      convs[convId] = { ...conv, messages: [...conv.messages, newMsg] };
      store.update({ conversations: convs });
    }
  };

  const handleFileChange = async (e: ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (!files || files.length === 0) return;

    const convId = store.activeConvId;
    if (!convId) { toast.showToast('Select a conversation first.', 'error'); return; }
    const conv = store.conversations[convId];
    if (!conv) return;

    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      appendLog('info', `Uploading ${file.name} (${(file.size / 1024).toFixed(1)} KB)…`);
      toast.showToast(`Uploading ${file.name}…`, 'success');

      const form = new FormData();
      form.append('file', file, file.name);

      let uploadRes;
      try {
        const res = await fetch('/api/attachments', {
          method: 'POST',
          headers: { 'Authorization': `Bearer ${store.token}` },
          body: form,
        });
        uploadRes = await res.json();
        if (!res.ok) {
          appendLog('err', `Upload failed: ${JSON.stringify(uploadRes)}`);
          toast.showToast(`Upload failed: ${uploadRes.message || res.status}`, 'error');
          continue;
        }
        appendLog('ok', `Uploaded ${file.name} → ${uploadRes.attachment_id}`);
      } catch (err: any) {
        appendLog('err', `Upload error: ${err.message}`);
        toast.showToast('Upload failed — network error', 'error');
        continue;
      }

      const displayText = `📎 ${file.name}`;
      const envelope = {
        conversation_id: convId,
        sender_user_id: store.myUserId,
        sender_device_id: store.myDeviceId,
        recipient_user_id: conv.recipientUserId,
        recipient_device_id: conv.recipientDeviceId || null,
        ciphertext: encodeMsg(displayText),
        protocol_header: {
          type: 'DoubleRatchet', ratchet_key: dummyKey32(), prev_chain_n: 0, msg_n: conv.messages.length,
        },
        attachment_id: uploadRes.attachment_id,
      };

      const msgRes = await apiFetch('POST', `/conversations/${convId}/messages`, { envelope }, store.token, appendLog);
      if (msgRes.ok) {
        const newMsg: TestMessage = {
          sender_user_id: store.myUserId,
          seq: msgRes.data?.seq,
          server_ts: msgRes.data?.server_ts,
          _text: displayText,
          _mine: true,
          attachment_id: uploadRes.attachment_id,
          attachment_name: file.name,
          attachment_type: file.type,
          attachment_size: file.size,
        };

        const convs = { ...store.conversations };
        convs[convId] = { ...convs[convId], messages: [...convs[convId].messages, newMsg] };
        store.update({ conversations: convs });
        toast.showToast(`Sent: ${file.name}`, 'success');
      }
    }
    e.target.value = '';
  };

  const pollMessages = async () => {
    const convId = store.activeConvId;
    if (!convId) return;
    const res = await apiFetch('GET', `/conversations/${convId}/messages?limit=50`, null, store.token, appendLog);
    if (!res.ok || !res.data?.messages) return;

    const convs = { ...store.conversations };
    const conv = convs[convId];
    if (!conv) return;

    const existing = new Set(conv.messages.map(m => m.seq));
    let added = false;
    const newMsgs = [...conv.messages];

    for (const m of res.data.messages) {
      if (!existing.has(m.seq)) {
        const senderStr = typeof m.sender_user_id === 'string' ? m.sender_user_id : (m.sender_user_id?.['0'] || '');
        newMsgs.push({
          ...m,
          _text: m.attachment_id ? `📎 attachment` : decodeMsg(m.ciphertext),
          _mine: senderStr === store.myUserId,
          attachment_id: m.attachment_id || null,
        });
        added = true;
      }
    }

    if (added) {
      newMsgs.sort((a, b) => (a.seq || 0) - (b.seq || 0));
      convs[convId] = { ...conv, messages: newMsgs };
      store.update({ conversations: convs });
    }
  };

  const activeConv = store.activeConvId ? store.conversations[store.activeConvId] : null;

  return (
    <div className="flex flex-1 overflow-hidden h-full">
      <aside className="w-[300px] shrink-0 flex flex-col bg-[#1a1d27] border-r border-[#2e3250] overflow-y-auto">
        {/* My Identity */}
        <div className="p-4 border-b border-[#2e3250]">
          <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] mb-3">My Identity</div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Bearer Token (JWT)</label>
            <Input
              placeholder="eyJ…"
              value={store.token}
              onChange={(e) => store.update({ token: e.target.value })}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">My User ID</label>
            <Input
              placeholder="user-alice"
              value={store.myUserId}
              onChange={(e) => store.update({ myUserId: e.target.value })}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">My Device ID (UUID)</label>
            <Input
              placeholder="auto-filled after register"
              value={store.myDeviceId}
              onChange={(e) => store.update({ myDeviceId: e.target.value })}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex gap-2">
            <Button onClick={handleRegisterDevice} className="flex-1 bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white h-8 text-[12px]">Register</Button>
            <Button variant="ghost" onClick={() => connectWs(false)} className="flex-1 h-8 text-[12px] bg-transparent border-[#2e3250] text-[#7b82a8] hover:border-[#5c6ef8] hover:text-[#e2e4f0]">Connect WS</Button>
          </div>
        </div>

        {/* New Conversation */}
        <div className="p-4 border-b border-[#2e3250]">
          <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] mb-3">New Conversation</div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Recipient User ID</label>
            <Input
              placeholder="user-bob"
              value={recipientUserId}
              onChange={(e) => setRecipientUserId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <div className="flex flex-col gap-1.5 mb-2.5">
            <label className="text-[11px] text-[#7b82a8]">Recipient Device ID</label>
            <Input
              placeholder="00000000-…"
              value={recipientDeviceId}
              onChange={(e) => setRecipientDeviceId(e.target.value)}
              className="bg-[#22263a] border-[#2e3250] text-[#e2e4f0] h-8 text-[13px]"
            />
          </div>
          <Button onClick={handleStartConversation} className="w-full bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white h-8 text-[12px]">Start Conversation</Button>
        </div>

        {/* Conversation List */}
        <div className="flex-1 flex flex-col min-h-[200px]">
          <div className="p-4 pb-2">
            <div className="text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8]">Conversations</div>
          </div>
          <div className="flex-1 overflow-y-auto px-2">
            {Object.values(store.conversations).length === 0 ? (
              <div className="p-2 text-center text-[12px] text-[#7b82a8]">No conversations yet</div>
            ) : (
              Object.values(store.conversations).map(c => {
                const isActive = c.id === store.activeConvId;
                const lastMsg = c.messages[c.messages.length - 1]?._text || c.id.slice(0, 8) + '…';
                return (
                  <div
                    key={c.id}
                    onClick={() => { store.update({ activeConvId: c.id }); pollMessages(); }}
                    className={`flex items-center gap-2.5 p-2 rounded-lg cursor-pointer border mb-1 transition-colors ${isActive ? 'bg-[#22263a] border-[#5c6ef8]' : 'border-transparent hover:bg-[#22263a]'
                      }`}
                  >
                    <div className="w-8 h-8 rounded-full bg-[#3d4ab0] flex items-center justify-center text-[13px] font-bold text-[#e2e4f0] shrink-0">
                      {c.recipientUserId.slice(0, 2).toUpperCase()}
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="text-[13px] font-medium text-[#e2e4f0] truncate">{c.recipientUserId}</div>
                      <div className="text-[11px] text-[#7b82a8] truncate">{lastMsg}</div>
                    </div>
                  </div>
                );
              })
            )}
          </div>
          <div className="p-4 border-t border-[#2e3250]">
            <Button variant="ghost" onClick={pollMessages} className="w-full h-8 text-[11px] bg-transparent border border-transparent text-[#7b82a8] hover:border-[#5c6ef8] hover:text-[#e2e4f0]">↻ Poll Messages</Button>
          </div>
        </div>
      </aside>

      <main className="flex-1 flex flex-col overflow-hidden bg-[#0f1117]">
        {!activeConv ? (
          <div className="flex-1 flex flex-col items-center justify-center gap-2 text-[#7b82a8] text-[14px]">
            <div className="text-[40px]">💬</div>
            <div>Select or start a conversation</div>
            <div className="text-[12px] text-center">Messages use dummy bytes as ciphertext<br />(E2E crypto omitted in dev client)</div>
          </div>
        ) : (
          <>
            <div className="px-5 h-[52px] flex items-center gap-3 border-b border-[#2e3250] shrink-0">
              <div className="w-8 h-8 rounded-full bg-[#3d4ab0] flex items-center justify-center text-[12px] font-bold text-[#e2e4f0]">
                {activeConv.recipientUserId.slice(0, 2).toUpperCase()}
              </div>
              <div>
                <div className="text-[15px] font-semibold text-[#e2e4f0]">{activeConv.recipientUserId}</div>
                <div className="text-[11px] text-[#7b82a8] font-mono">{activeConv.id}</div>
              </div>
            </div>

            <div ref={msgListRef} className="flex-1 overflow-y-auto p-5 flex flex-col gap-3">
              {activeConv.messages.length === 0 ? (
                <div className="m-auto text-[#7b82a8] text-[13px]">No messages yet</div>
              ) : (
                activeConv.messages.map((m, i) => {
                  const isMine = m._mine;
                  const sender = typeof m.sender_user_id === 'string' ? m.sender_user_id : '?';
                  const time = m.server_ts ? new Date(Number(m.server_ts)).toLocaleTimeString('en', { hour: '2-digit', minute: '2-digit' }) : '';
                  const aid = m.attachment_id;
                  const showText = !aid || (m._text && !m._text.startsWith('📎 '));

                  return (
                    <div key={i} className={`flex flex-col max-w-[68%] ${isMine ? 'self-end items-end' : 'self-start items-start'}`}>
                      <div className="text-[10px] text-[#7b82a8] mb-1">{isMine ? 'You' : sender}{time ? ' · ' + time : ''}</div>
                      <div className={`px-3.5 py-2 rounded-[14px] text-[14px] leading-relaxed break-word ${isMine ? 'bg-[#5c6ef8] text-white rounded-br-[4px]' : 'bg-[#22263a] text-[#e2e4f0] rounded-bl-[4px]'}`}>
                        {showText ? (m._text || '[empty]') : ''}

                        {aid && (
                          <a
                            href={`/api/attachments/${aid}`}
                            target="_blank"
                            download={m.attachment_name || 'attachment'}
                            className={`flex items-center gap-1.5 mt-1.5 px-3 py-1.5 rounded-lg text-[12px] font-medium no-underline ${isMine ? 'bg-white/15 text-white hover:bg-white/20' : 'bg-[#1a1d27] text-[#e2e4f0] hover:bg-[#2e3250]'}`}
                          >
                            📎 {m.attachment_name || m._text?.replace('📎 ', '') || 'attachment'}
                            {m.attachment_size ? ` · ${(m.attachment_size / 1024).toFixed(1)} KB` : ''}
                          </a>
                        )}
                      </div>
                      {m.seq != null && <div className={`text-[10px] text-[#7b82a8] mt-0.5 ${isMine ? 'text-right' : ''}`}>seq {m.seq}</div>}
                    </div>
                  );
                })
              )}
            </div>

            <div className="p-3.5 px-5 border-t border-[#2e3250] flex gap-2.5 shrink-0 bg-[#0f1117]">
              <Input
                placeholder="Type a message…"
                value={msgInput}
                onChange={(e) => setMsgInput(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
                className="flex-1 bg-[#22263a] border-[#2e3250] text-[#e2e4f0] text-[14px] px-3.5"
              />
              <label className="flex items-center justify-center px-4 py-2 border border-[#2e3250] rounded-md bg-transparent text-[#7b82a8] hover:border-[#5c6ef8] hover:text-[#e2e4f0] cursor-pointer transition-colors text-[14px]">
                📎
                <input type="file" className="hidden" multiple onChange={handleFileChange} />
              </label>
              <Button onClick={handleSendMessage} className="bg-[#5c6ef8] hover:bg-[#3d4ab0] text-white px-5">Send</Button>
            </div>
          </>
        )}
      </main>

      <DevLog logs={logs} onClear={clearLog} />
    </div>
  );
}
