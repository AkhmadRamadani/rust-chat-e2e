import React from 'react';

export function Card({
  className = '',
  children,
}: {
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={`bg-surface2 border border-border rounded-lg overflow-hidden ${className}`}>
      {children}
    </div>
  );
}
