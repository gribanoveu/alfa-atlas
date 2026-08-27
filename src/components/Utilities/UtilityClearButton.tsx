import type { ReactNode } from "react";
import { X } from "lucide-react";
import "./UtilityClearButton.css";

type UtilityClearButtonProps = {
  onClear: () => void;
  disabled?: boolean;
  label?: string;
};

export function UtilityClearButton({
  onClear,
  disabled = false,
  label = "Очистить",
}: UtilityClearButtonProps) {
  return (
    <button
      type="button"
      className="utility-clear-btn"
      onClick={onClear}
      disabled={disabled}
      aria-label={label}
      title={label}
    >
      <X size={13} strokeWidth={2} aria-hidden />
    </button>
  );
}

type UtilityFieldShellProps = {
  onClear: () => void;
  clearDisabled?: boolean;
  clearLabel?: string;
  variant?: "block" | "inline";
  children: ReactNode;
};

export function UtilityFieldShell({
  onClear,
  clearDisabled = false,
  clearLabel,
  variant = "block",
  children,
}: UtilityFieldShellProps) {
  return (
    <div className={`utility-field-shell${variant === "inline" ? " is-inline" : ""}`}>
      {children}
      <UtilityClearButton onClear={onClear} disabled={clearDisabled} label={clearLabel} />
    </div>
  );
}

type UtilityLabeledFieldProps = {
  label: string;
  htmlFor?: string;
  onClear: () => void;
  clearDisabled?: boolean;
  clearLabel?: string;
  children: ReactNode;
};

export function UtilityLabeledField({
  label,
  htmlFor,
  onClear,
  clearDisabled = false,
  clearLabel,
  children,
}: UtilityLabeledFieldProps) {
  const labelNode = htmlFor ? (
    <label className="utility-field-label" htmlFor={htmlFor}>
      {label}
    </label>
  ) : (
    <span className="utility-field-label">{label}</span>
  );

  return (
    <div className="utility-field">
      {labelNode}
      <UtilityFieldShell onClear={onClear} clearDisabled={clearDisabled} clearLabel={clearLabel}>
        {children}
      </UtilityFieldShell>
    </div>
  );
}
