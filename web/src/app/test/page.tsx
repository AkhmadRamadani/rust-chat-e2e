'use client';

import React, { useState, useEffect } from 'react';
import { ChatTab } from './ChatTab';
import { UsersTab } from './UsersTab';
type TabType = 'chat' | 'users';

export default function TestClientPage() {
  const [activeTab, setActiveTab] = useState<TabType>('chat');
  const [globalStatus, setGlobalStatus] = useState({ ok: false, label: 'checking…' });

  // Add the Google Font used by the original client
  useEffect(() => {
    const link = document.createElement('link');
    link.href = 'https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap';
    link.rel = 'stylesheet';
    document.head.appendChild(link);
    return () => {
      document.head.removeChild(link);
    };
  }, []);

  return (
    <div
      className="flex flex-col h-[100dvh] w-full overflow-hidden text-[#e2e4f0] bg-[#0f1117]"
      style={{ fontFamily: "'Inter', system-ui, sans-serif" }}
    >
      <header className="flex items-center gap-3 px-5 h-[52px] bg-[#1a1d27] border-b border-[#2e3250] shrink-0">
        <h1 className="text-[15px] font-semibold m-0">rust-e2e-chat</h1>
        <span className="text-[10px] bg-[#3d4ab0] text-[#a5aff9] px-2 py-0.5 rounded-full font-semibold uppercase tracking-wide">
          dev client (react)
        </span>
        <div className="flex-1" />
        <a href="/docs" className="text-[13px] text-[#7b82a8] hover:text-[#e2e4f0] transition-colors flex items-center gap-1.5 mr-2">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 19.5v-15A2.5 2.5 0 0 1 6.5 2H20v20H6.5a2.5 2.5 0 0 1 0-5H20"/></svg>
          Docs
        </a>
        <div className={`w-2 h-2 rounded-full ${globalStatus.ok ? 'bg-[#3ecf8e]' : 'bg-[#f87171]'}`} />
        <span className="text-[12px] text-[#7b82a8]">{globalStatus.label}</span>
      </header>

      <nav className="flex bg-[#1a1d27] border-b border-[#2e3250] shrink-0 px-5 gap-1">
        <button
          onClick={() => setActiveTab('chat')}
          className={`px-4 py-2.5 text-[13px] font-medium border-b-2 transition-colors ${
            activeTab === 'chat'
              ? 'text-[#5c6ef8] border-[#5c6ef8]'
              : 'text-[#7b82a8] border-transparent hover:text-[#e2e4f0]'
          }`}
        >
          💬 Chat
        </button>
        <button
          onClick={() => setActiveTab('users')}
          className={`px-4 py-2.5 text-[13px] font-medium border-b-2 transition-colors ${
            activeTab === 'users'
              ? 'text-[#5c6ef8] border-[#5c6ef8]'
              : 'text-[#7b82a8] border-transparent hover:text-[#e2e4f0]'
          }`}
        >
          👤 Users
        </button>
      </nav>

      <div className="flex-1 flex overflow-hidden min-h-0 relative">
        {activeTab === 'chat' && <ChatTab setGlobalStatus={setGlobalStatus} />}
        {activeTab === 'users' && <UsersTab onSwitchToChat={() => setActiveTab('chat')} />}
      </div>
    </div>
  );
}
