import React from 'react';

interface BadgeProps {
  variant: 'active' | 'inactive' | 'pending';
  children: React.ReactNode;
  className?: string;
}

export function Badge({ variant, children, className = '' }: BadgeProps) {
  const variants = {
    active: 'bg-success/20 text-success border-success/30',
    inactive: 'bg-danger/20 text-danger border-danger/30',
    pending: 'bg-warning/20 text-warning border-warning/30',
  };

  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-medium border ${variants[variant]} ${className}`}
    >
      {children}
    </span>
  );
}
