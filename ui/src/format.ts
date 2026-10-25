export function bytes(n: number | null | undefined): string {
  if (n == null || !isFinite(n)) return "?";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1000 && i < u.length - 1) {
    v /= 1000;
    i++;
  }
  return `${v >= 100 || i === 0 ? v.toFixed(0) : v.toFixed(1)} ${u[i]}`;
}

export function speed(bps: number): string {
  return `${bytes(bps)}/s`;
}

export function duration(s: number | null | undefined): string {
  if (s == null || !isFinite(s)) return "";
  s = Math.max(0, Math.round(s));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function price(p: { amount: number; currency: string } | null | undefined): string {
  if (!p || !Number.isFinite(p.amount)) return "";
  try {
    return new Intl.NumberFormat(undefined, { style: "currency", currency: p.currency.toUpperCase() }).format(p.amount / 100);
  } catch {
    return `${(p.amount / 100).toFixed(2)} ${p.currency.toUpperCase()}`;
  }
}

export function statusLabel(status: string): string {
  switch (status) {
    case "released":
      return "Released";
    case "early_access":
      return "Early Access";
    case "in_development":
      return "In Development";
    case "coming_soon":
      return "Coming Soon";
    default:
      return status.replace(/_/g, " ");
  }
}

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0]!.toUpperCase())
    .join("");
}
