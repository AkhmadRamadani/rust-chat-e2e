'use client';

import React, { useEffect, useRef } from 'react';

export type LogLevel = 'info' | 'ok' | 'err' | 'warn';

export interface LogEntry {
  id: string;
  level: LogLevel;
  timestamp: string;
  message: string;
}

export function useDevLog() {
  const [logs, setLogs] = React.useState<LogEntry[]>([]);

  const appendLog = (level: LogLevel, message: string) => {
    const ts = new Date().toLocaleTimeString('en', {
      hour12: false,
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    setLogs((prev) => [
      ...prev,
      {
        id: Math.random().toString(36).substring(2, 9),
        level,
        timestamp: ts,
        message,
      },
    ]);
  };

  const clearLog = () => setLogs([]);

  return { logs, appendLog, clearLog };
}

export function DevLog({ logs, onClear }: { logs: LogEntry[]; onClear: () => void }) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new logs arrive
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs]);

  return (
    <aside className="w-[280px] shrink-0 flex flex-col bg-[#1a1d27] border-l border-[#2e3250] overflow-hidden">
      <div className="px-4 py-3 text-[10px] font-bold uppercase tracking-[0.8px] text-[#7b82a8] border-b border-[#2e3250]">
        Request Log
      </div>
      
      <div
        ref={scrollRef}
        className="flex-1 overflow-y-auto p-3 text-[11px] font-mono flex flex-col gap-1"
      >
        {logs.map((log) => {
          let bgColor = '';
          let textColor = '';
          
          if (log.level === 'info') {
            bgColor = 'bg-[#1e2038]';
            textColor = 'text-[#a5aff9]';
          } else if (log.level === 'ok') {
            bgColor = 'bg-[#0d2b1e]';
            textColor = 'text-[#3ecf8e]';
          } else if (log.level === 'err') {
            bgColor = 'bg-[#2b0d0d]';
            textColor = 'text-[#f87171]';
          } else if (log.level === 'warn') {
            bgColor = 'bg-[#2b1f0d]';
            textColor = 'text-[#fbbf24]';
          }

          return (
            <div
              key={log.id}
              className={`px-2 py-1 rounded-[4px] leading-relaxed break-all ${bgColor} ${textColor}`}
            >
              <span className="text-[#7b82a8] mr-1.5">{log.timestamp}</span>
              {log.message}
            </div>
          );
        })}
      </div>
      
      <div className="p-2 border-t border-[#2e3250]">
        <button
          onClick={onClear}
          className="w-full text-[11px] py-1 text-[#7b82a8] hover:text-[#e2e4f0] border border-transparent hover:border-[#5c6ef8] transition-colors rounded-[8px]"
        >
          Clear
        </button>
      </div>
    </aside>
  );
}
