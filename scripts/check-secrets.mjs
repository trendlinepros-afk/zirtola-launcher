// Pre-push / CI guard: fail if anything that looks like a secret or personal
// data is about to be committed. Scans every file git would include (tracked
// plus untracked-but-not-ignored).
//   node scripts/check-secrets.mjs
import { execFileSync } from "node:child_process";
import { readFileSync, statSync } from "node:fs";

const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"], { encoding: "utf8" })
  .split("\0")
  .filter(Boolean)
  .filter((f) => !/\.(png|ico|bmp|icns|jpg|lock)$/i.test(f) && f !== "package-lock.json" && f !== "scripts/check-secrets.mjs");

const RULES = [
  ["PEM private key", /-----BEGIN [A-Z ]*PRIVATE KEY-----/],
  ["minisign/Tauri updater secret key", /rsign encrypted secret key|untrusted comment: minisign encrypted secret key/i],
  ["private JWK ('d' member)", /"d"\s*:\s*"[A-Za-z0-9_-]{40,}"/],
  ["GitHub token", /\b(ghp|gho|ghu|ghs|ghr|github_pat)_[A-Za-z0-9_]{20,}/],
  ["AWS access key", /\bAKIA[0-9A-Z]{16}\b/],
  ["Stripe secret key", /\b(sk|rk)_(live|test)_[A-Za-z0-9]{16,}/],
  ["Azure client secret assignment", /AZURE_CLIENT_SECRET\s*[=:]\s*["']?[A-Za-z0-9~._-]{20,}/],
  ["password assignment", /\b(password|passwd|secret)\s*[=:]\s*["'][^"'\s$<{]{12,}["']/i],
  ["PFX/P12 base64 blob", /MII[A-Za-z0-9+/]{200,}/],
];
// Email addresses are personal data unless they are clearly placeholders.
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g;
const ALLOWED_EMAIL = /@(example\.(com|test|org)|users\.noreply\.github\.com|anthropic\.com)$|^noreply@/i;

let problems = 0;
for (const f of files) {
  let text;
  try {
    if (statSync(f).size > 2_000_000) continue;
    text = readFileSync(f, "utf8");
  } catch {
    continue;
  }
  for (const [name, re] of RULES) {
    if (re.test(text)) {
      console.error(`SECRET? ${f}: ${name}`);
      problems++;
    }
  }
  for (const m of text.match(EMAIL) ?? []) {
    if (!ALLOWED_EMAIL.test(m) && !/\.(png|jpg|svg|ico|bmp|webp)$/i.test(m)) {
      console.error(`PERSONAL DATA? ${f}: email address ${m}`);
      problems++;
    }
  }
}
if (problems) {
  console.error(`\n${problems} potential secret(s) or personal data found. Remove them before committing.`);
  process.exit(1);
}
console.log(`check-secrets: ${files.length} files clean`);
