import React from 'react';

interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
}

export const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className = '', label, error, ...props }, ref) => {
    return (
      <div className="flex flex-col gap-1.5 w-full">
        {label && (
          <label className="text-sm font-medium text-text-dim">
            {label}
          </label>
        )}
        <input
          ref={ref}
          className={`
            w-full rounded bg-surface border px-3 py-2 text-text placeholder-text-dim
            focus:outline-none focus:border-accent
            disabled:opacity-50 disabled:bg-surface2
            ${error ? 'border-danger focus:border-danger' : 'border-border'}
            ${className}
          `}
          {...props}
        />
        {error && <span className="text-sm text-danger">{error}</span>}
      </div>
    );
  }
);

Input.displayName = 'Input';
