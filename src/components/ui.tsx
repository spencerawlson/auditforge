/** Shared presentational primitives. Styling lives in `styles/global.css`. */

import type { ButtonHTMLAttributes, InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";
import { useId } from "react";

// ---------------------------------------------------------------- button

type ButtonVariant = "default" | "primary" | "danger" | "ghost";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: "sm" | "md" | "lg";
  block?: boolean;
  busy?: boolean;
}

export function Button({
  variant = "default",
  size = "md",
  block = false,
  busy = false,
  className = "",
  children,
  disabled,
  ...rest
}: ButtonProps) {
  const classes = [
    "btn",
    variant !== "default" ? `btn--${variant}` : "",
    size !== "md" ? `btn--${size}` : "",
    block ? "btn--block" : "",
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button className={classes} disabled={disabled || busy} {...rest}>
      {busy && <span className="spinner" aria-hidden="true" />}
      {children}
    </button>
  );
}

// ---------------------------------------------------------------- fields

interface FieldProps {
  label: string;
  children: ReactNode;
  help?: string;
  error?: string;
  required?: boolean;
  htmlFor?: string;
}

export function Field({ label, children, help, error, required, htmlFor }: FieldProps) {
  return (
    <div className="field">
      {/* The required marker is drawn by CSS rather than put in the label text, so the
          accessible name stays "Username" and not "Username*". The input's own `required`
          attribute is what actually conveys it to assistive technology. */}
      <label
        className={`field__label${required ? " field__label--required" : ""}`}
        htmlFor={htmlFor}
      >
        {label}
      </label>
      {help && <div className="field__help">{help}</div>}
      {children}
      {error && <div className="field__error">{error}</div>}
    </div>
  );
}

interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  label: string;
  help?: string;
  error?: string;
  onValueChange?: (value: string) => void;
}

export function TextField({
  label,
  help,
  error,
  onValueChange,
  required,
  ...rest
}: TextFieldProps) {
  const id = useId();
  return (
    <Field label={label} help={help} error={error} required={required} htmlFor={id}>
      <input
        id={id}
        className={`input${error ? " input--invalid" : ""}`}
        aria-invalid={error ? true : undefined}
        onChange={(event) => onValueChange?.(event.target.value)}
        {...rest}
      />
    </Field>
  );
}

interface TextAreaFieldProps
  extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "onChange"> {
  label: string;
  help?: string;
  error?: string;
  tall?: boolean;
  onValueChange?: (value: string) => void;
}

export function TextAreaField({
  label,
  help,
  error,
  tall,
  onValueChange,
  required,
  ...rest
}: TextAreaFieldProps) {
  const id = useId();
  return (
    <Field label={label} help={help} error={error} required={required} htmlFor={id}>
      <textarea
        id={id}
        className={`textarea${tall ? " textarea--tall" : ""}${error ? " textarea--invalid" : ""}`}
        aria-invalid={error ? true : undefined}
        onChange={(event) => onValueChange?.(event.target.value)}
        {...rest}
      />
    </Field>
  );
}

interface SelectFieldProps<T extends string>
  extends Omit<SelectHTMLAttributes<HTMLSelectElement>, "onChange" | "value"> {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  help?: string;
  error?: string;
  onValueChange: (value: T) => void;
}

export function SelectField<T extends string>({
  label,
  value,
  options,
  help,
  error,
  onValueChange,
  required,
  ...rest
}: SelectFieldProps<T>) {
  const id = useId();
  return (
    <Field label={label} help={help} error={error} required={required} htmlFor={id}>
      <select
        id={id}
        className="select"
        value={value}
        onChange={(event) => onValueChange(event.target.value as T)}
        {...rest}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </Field>
  );
}

interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  hint?: string;
  disabled?: boolean;
}

export function Checkbox({ checked, onChange, label, hint, disabled }: CheckboxProps) {
  return (
    <label className="checkbox">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="checkbox__text">
        {label}
        {hint && <span className="checkbox__hint">{hint}</span>}
      </span>
    </label>
  );
}

// ---------------------------------------------------------------- containers

export function Card({
  title,
  actions,
  children,
  flush = false,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  flush?: boolean;
}) {
  return (
    <section className="card">
      {(title || actions) && (
        <header className="card__header">
          {title && <h2 className="card__title">{title}</h2>}
          {actions && <div className="card__actions">{actions}</div>}
        </header>
      )}
      <div className={flush ? "card__body card__body--flush" : "card__body"}>{children}</div>
    </section>
  );
}

export function PageHeader({
  title,
  subtitle,
  actions,
}: {
  title: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="page-header">
      <div className="page-header__text">
        <h1>{title}</h1>
        {subtitle && <div className="page-header__sub">{subtitle}</div>}
      </div>
      {actions && <div className="page-header__actions">{actions}</div>}
    </header>
  );
}

export function EmptyState({
  title,
  hint,
  action,
}: {
  title: string;
  hint?: string;
  action?: ReactNode;
}) {
  return (
    <div className="empty">
      <div className="empty__title">{title}</div>
      {hint && <p className="empty__hint">{hint}</p>}
      {action}
    </div>
  );
}

export function Notice({
  tone = "info",
  title,
  children,
}: {
  tone?: "info" | "warning" | "danger" | "success" | "neutral";
  title?: string;
  children: ReactNode;
}) {
  return (
    <div className={`notice${tone === "neutral" ? "" : ` notice--${tone}`}`} role="note">
      <div>
        {title && <div className="notice__title">{title}</div>}
        <div>{children}</div>
      </div>
    </div>
  );
}

export function Spinner({ label = "Loading" }: { label?: string }) {
  return (
    <span className="row" role="status">
      <span className="spinner" aria-hidden="true" />
      <span className="sr-only">{label}</span>
    </span>
  );
}

export function LoadingBlock({ label = "Loading" }: { label?: string }) {
  return (
    <div className="empty">
      <span className="spinner" aria-hidden="true" />
      <span className="muted small">{label}</span>
    </div>
  );
}

export function Tabs<T extends string>({
  tabs,
  active,
  onChange,
}: {
  tabs: ReadonlyArray<{ id: T; label: string; badge?: ReactNode }>;
  active: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="tabs" role="tablist">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          type="button"
          aria-selected={tab.id === active}
          className={`tab${tab.id === active ? " tab--active" : ""}`}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
          {tab.badge}
        </button>
      ))}
    </div>
  );
}

export function KeyValues({ items }: { items: Array<[string, ReactNode]> }) {
  return (
    <dl className="kv">
      {items.map(([key, value]) => (
        <div key={key} style={{ display: "contents" }}>
          <dt>{key}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * A save bar that follows the user down a settings screen while changes are pending.
 *
 * Exists because a Save button sitting at the bottom of one card is easy to miss when the
 * control you just changed is at the top of another - which is how a selected AI provider
 * could look active while never having been written.
 */
export function UnsavedBar({
  dirty,
  busy,
  label,
  onSave,
  onDiscard,
}: {
  dirty: boolean;
  busy?: boolean;
  label: string;
  onSave: () => void;
  onDiscard: () => void;
}) {
  if (!dirty) return null;
  return (
    <div className="savebar" role="status">
      <span className="savebar__text">You have unsaved changes.</span>
      <Button variant="ghost" size="sm" onClick={onDiscard} disabled={busy}>
        Discard
      </Button>
      <Button variant="primary" busy={busy} onClick={onSave}>
        {label}
      </Button>
    </div>
  );
}

export function Stat({ value, label }: { value: ReactNode; label: string }) {
  return (
    <div className="card stat">
      <div className="stat__value">{value}</div>
      <div className="stat__label">{label}</div>
    </div>
  );
}
