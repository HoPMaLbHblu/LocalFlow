// Building blocks shared by the guide in every language.

export type Block =
  | { kind: "text"; text: string }
  | { kind: "code"; code: string; runnable?: boolean }
  | { kind: "tip"; text: string }
  | { kind: "warning"; text: string };

export interface Lesson {
  id: string;
  title: string;
  summary: string;
  blocks: Block[];
}

export const t = (text: string): Block => ({ kind: "text", text });
export const code = (source: string, runnable = true): Block => ({ kind: "code", code: source, runnable });
export const tip = (text: string): Block => ({ kind: "tip", text });
export const warning = (text: string): Block => ({ kind: "warning", text });

export type HintId =
  | "fieldCall"
  | "blockedGlobal"
  | "globalCall"
  | "nilValue"
  | "concatNil"
  | "concatType"
  | "arith"
  | "missingEnd"
  | "missingThen"
  | "missingDo"
  | "missingEquals"
  | "unfinishedString"
  | "missingBrace"
  | "missingParen"
  | "unexpectedSymbol"
  | "accessDenied"
  | "notFound"
  | "destExists"
  | "timedOut"
  | "missingRun"
  | "appNotFound"
  | "timeLimit"
  | "badArgument"
  | "stepMissing";

/** A translation of the guide. Anything missing falls back to English. */
export interface GuideTranslation {
  /** Function docs by name, e.g. "fs.list". */
  api: Record<string, { summary: string; returns?: string }>;
  /** Snippets by their English title. */
  snippets: Record<string, { title: string; description: string }>;
  lessons: Lesson[];
  /** `fsNames` is the list of fs.* functions, for the "misspelled function" hint. */
  hints: Record<HintId, (m: RegExpMatchArray, fsNames: string) => string>;
}
