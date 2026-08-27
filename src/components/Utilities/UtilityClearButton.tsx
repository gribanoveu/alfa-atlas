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

type UtilityInputHeadProps = {
  label: string;
  htmlFor?: string;
  onClear: () => void;
  clearDisabled?: boolean;
  clearLabel?: string;
};

export function UtilityInputHead({
  label,
  htmlFor,
  onClear,
  clearDisabled = false,
  clearLabel,
}: UtilityInputHeadProps) {
  const labelNode = htmlFor ? (
    <label className="utility-input-head-label" htmlFor={htmlFor}>
      {label}
    </label>
  ) : (
    <span className="utility-input-head-label">{label}</span>
  );

  return (
    <div className="utility-input-head">
      {labelNode}
      <UtilityClearButton onClear={onClear} disabled={clearDisabled} label={clearLabel} />
    </div>
  );
}
