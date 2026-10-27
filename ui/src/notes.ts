// Tiny Markdown subset for release notes: headings, bullet lists, paragraphs.
// Wrapped lines join the bullet or paragraph they belong to (release notes
// are written wrapped at ~80 columns). Inline formatting is left to the
// renderer. Pure, so it can be unit-tested (notes.test.ts).

export type NotesBlock = { kind: "h"; text: string } | { kind: "p"; text: string } | { kind: "ul"; items: string[] };

// Section names release notes use without "#": a line that is only one of
// these (optionally bold, optionally with a colon) is a heading, even straight
// after a bullet, never a wrapped continuation of it.
const SECTIONS =
  /^\s*(?:\*\*|__)?\s*(new|added|changed|changes|fixed|fixes|bug fixes|removed|improved|improvements|balance|performance|security|deprecated|known issues|notes?)\s*:?\s*(?:\*\*|__)?\s*:?\s*$/i;

function sectionHeading(line: string): [string, string] | null {
  const m = SECTIONS.exec(line);
  if (!m) return null;
  const word = m[1]!.trim();
  return [line, word.charAt(0).toUpperCase() + word.slice(1)];
}

export function parseNotes(md: string): NotesBlock[] {
  const out: NotesBlock[] = [];
  let open: NotesBlock | null = null; // paragraph or list still taking lines
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trimEnd();
    if (!line.trim()) {
      open = null;
      continue;
    }
    const item = /^\s*[-*]\s+(.*)$/.exec(line);
    if (item) {
      if (open?.kind !== "ul") {
        open = { kind: "ul", items: [] };
        out.push(open);
      }
      open.items.push(item[1]!.trim());
      continue;
    }
    const head = /^#{1,6}\s+(.*)$/.exec(line) ?? sectionHeading(line);
    if (head) {
      out.push({ kind: "h", text: head[1]!.trim() });
      open = null;
      continue;
    }
    // A wrapped line: continues the last bullet or the open paragraph.
    if (open?.kind === "ul") {
      open.items[open.items.length - 1] += ` ${line.trim()}`;
    } else if (open?.kind === "p") {
      open.text += ` ${line.trim()}`;
    } else {
      open = { kind: "p", text: line.trim() };
      out.push(open);
    }
  }
  return out;
}
