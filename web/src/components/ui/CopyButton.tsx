'use client';
import React, { useState } from 'react';

export function CopyButton({ text, className = '' }: { text: string; className?: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy', err);
    }
  };

  return (
    <button
      onClick={handleCopy}
      className={`text-xs font-medium px-2 py-1 rounded border transition-colors focus:outline-none
        ${copied 
          ? 'bg-success/20 text-success border-success/30' 
          : 'bg-surface2 text-text-dim border-border hover:bg-border hover:text-text'
        } ${className}`}
    >
      {copied ? 'Copied!' : 'Copy'}
    </button>
  );
}
