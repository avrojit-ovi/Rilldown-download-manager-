import { cn } from "@/lib/utils";

interface ProgressProps {
  value: number;
  indeterminate?: boolean;
  tone?: "accent" | "danger" | "muted";
  className?: string;
  label?: string;
}

export function Progress({
  value,
  indeterminate = false,
  tone = "accent",
  className,
  label,
}: ProgressProps) {
  const clamped = Math.min(Math.max(value, 0), 1);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={indeterminate ? undefined : Math.round(clamped * 100)}
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-surface-2", className)}
    >
      <div
        className={cn(
          "h-full rounded-full transition-[width] duration-300",
          tone === "accent" && "bg-accent",
          tone === "danger" && "bg-danger",
          tone === "muted" && "bg-muted/60",
          indeterminate && "animate-pulse",
        )}
        style={{ width: indeterminate ? "35%" : `${clamped * 100}%` }}
      />
    </div>
  );
}
