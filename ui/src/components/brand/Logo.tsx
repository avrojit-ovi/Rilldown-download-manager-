import { cn } from "@/lib/utils";

export function LogoMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 32 32"
      aria-hidden="true"
      className={cn("h-8 w-8", className)}
      role="img"
    >
      <rect x="0.5" y="0.5" width="31" height="31" rx="8.5" fill="#0E3A39" />
      <rect x="7.5" y="7.5" width="3.1" height="11.5" rx="1.55" fill="#7CE7D6" />
      <rect x="14.45" y="7.5" width="3.1" height="15" rx="1.55" fill="#EEFFFA" />
      <rect x="21.4" y="7.5" width="3.1" height="12.7" rx="1.55" fill="#4AC1B0" />
      <rect x="8.2" y="23.2" width="15.6" height="2.1" rx="1.05" fill="#7CE7D6" />
    </svg>
  );
}

export function Wordmark() {
  return (
    <div className="min-w-0">
      <div className="text-[15px] font-semibold leading-4 tracking-tight text-ink">Rilldown</div>
      <div className="mt-1 text-[9px] font-medium uppercase leading-none tracking-[0.18em] text-muted">
        Download manager
      </div>
    </div>
  );
}
