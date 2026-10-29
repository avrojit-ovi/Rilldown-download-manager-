import type { LucideIcon } from "lucide-react";
import type { ButtonHTMLAttributes } from "react";

import { cn } from "@/lib/utils";

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  label: string;
  icon: LucideIcon;
  active?: boolean;
}

export function IconButton({ label, icon: Icon, active = false, className, ...props }: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active || undefined}
      className={cn(
        "inline-flex h-8 w-8 items-center justify-center rounded-lg text-muted transition-colors hover:bg-surface-2 hover:text-ink disabled:pointer-events-none disabled:opacity-35",
        active && "bg-accent-soft text-accent hover:bg-accent-soft",
        className,
      )}
      {...props}
    >
      <Icon className="h-4 w-4" strokeWidth={1.9} aria-hidden="true" />
    </button>
  );
}
