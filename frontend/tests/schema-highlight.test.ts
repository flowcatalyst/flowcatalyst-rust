/**
 * The schema viewer inserts highlightJson's output with v-html; the schema is
 * stored data, so no input text may reach the page as markup (bug 4 in
 * docs/ui/README.md, docs/platform-bug-fixes.md).
 */

import { describe, expect, it } from "vitest";
import {
	highlightJava,
	highlightJson,
	highlightPhp,
	highlightPython,
	highlightTypeScript,
} from "@/utils/schema-highlight";

const hostile = [
	'{"title": "<img src=x onerror=alert(1)>"}',
	'{"<script>alert(1)</script>": true}',
	"<svg onload=alert(1)>",
	'"</span><b>x</b>"',
	'{"a": "&lt;already&gt; & raw"}',
];

// Only the highlighter's own <span class="hl-…"> wrappers may be markup.
function stripOwnMarkup(html: string): string {
	return html.replace(/<span class="hl-[a-z]+">/g, "").replace(/<\/span>/g, "");
}

describe("highlightJson", () => {
	it("escapes markup in keys, strings and text outside tokens", () => {
		for (const input of hostile) {
			const out = stripOwnMarkup(highlightJson(input));
			expect(out).not.toMatch(/[<>]/);
		}
		expect(highlightJson('{"t": "<b>"}')).toBe(
			'{<span class="hl-key">"t"</span>: <span class="hl-str">"&lt;b&gt;"</span>}',
		);
	});

	it("escapes ampersands so entities in data stay text", () => {
		expect(highlightJson('"&lt;"')).toBe(
			'<span class="hl-str">"&amp;lt;"</span>',
		);
	});

	it("is unchanged for JSON without & < >", () => {
		expect(highlightJson('{"a": 1, "b": [true, null, "x"]}')).toBe(
			'{<span class="hl-key">"a"</span>: <span class="hl-num">1</span>, ' +
				'<span class="hl-key">"b"</span>: [<span class="hl-kw">true</span>, ' +
				'<span class="hl-kw">null</span>, <span class="hl-str">"x"</span>]}',
		);
	});
});

describe("code highlighters", () => {
	it("escape their input", () => {
		for (const hl of [
			highlightTypeScript,
			highlightPhp,
			highlightPython,
			highlightJava,
		]) {
			for (const input of hostile) {
				expect(stripOwnMarkup(hl(input))).not.toMatch(/[<>]/);
			}
		}
	});
});
