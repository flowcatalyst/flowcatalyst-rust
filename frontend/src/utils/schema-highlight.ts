/**
 * Syntax Highlighting
 *
 * Lightweight single-pass regex-based highlighting for JSON, PHP, TypeScript, Python, and Java.
 * Each language uses a single combined regex so that matched tokens never
 * interfere with each other (no sequential replacement corruption).
 */

function escapeHtml(text: string): string {
	return text
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;");
}

function wrap(cls: string, text: string): string {
	return `<span class="${cls}">${text}</span>`;
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

// The result is inserted with v-html, and the input is stored data (any
// principal that can add a schema version writes it), so & < > are escaped
// before tokenising. Quotes stay raw: the tokenizer needs them, and the output
// is element content, never an attribute value. Same as the Go port
// (internal/schemacodegen/highlight.go).
function escapeJsonText(text: string): string {
	return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export function highlightJson(json: string): string {
	return escapeJsonText(json).replace(
		/("(?:\\.|[^"\\])*")\s*(:)|("(?:\\.|[^"\\])*")|(true|false|null)|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g,
		(match, key, colon, str, keyword, num) => {
			if (key) return `${wrap("hl-key", key)}${colon}`;
			if (str) return wrap("hl-str", str);
			if (keyword) return wrap("hl-kw", keyword);
			if (num) return wrap("hl-num", num);
			return match;
		},
	);
}

// ---------------------------------------------------------------------------
// TypeScript — single-pass
// ---------------------------------------------------------------------------

const TS_KEYWORDS = new Set([
	"export", "interface", "type", "const", "let", "var", "function", "return",
	"import", "from", "readonly", "extends", "implements", "class", "new",
	"typeof", "keyof", "in", "of", "as",
]);
const TS_TYPES = new Set([
	"string", "number", "boolean", "null", "undefined", "unknown", "void",
	"never", "any", "Record", "Array", "Partial", "Required", "Pick", "Omit",
]);

const TS_RE = new RegExp([
	/(\/\/.*)/.source,                           // comment
	/("(?:\\.|[^"\\])*")/.source,                // string (pre-escape: won't appear after escapeHtml)
	/(&quot;(?:[^&]|&(?!quot;))*?&quot;)/.source, // string (post-escape)
	/('(?:\\'|[^'])*')/.source,                  // single-quoted string
	/(\b\d+(?:\.\d+)?\b)/.source,               // number
	/(\b[A-Za-z_]\w*\b)/.source,                // identifier
].join("|"), "g");

export function highlightTypeScript(code: string): string {
	const escaped = escapeHtml(code);
	return escaped.replace(TS_RE, (match, comment, dstr, estr, sstr, num, ident) => {
		if (comment) return wrap("hl-comment", comment);
		if (dstr) return wrap("hl-str", dstr);
		if (estr) return wrap("hl-str", estr);
		if (sstr) return wrap("hl-str", sstr);
		if (num) return wrap("hl-num", num);
		if (ident) {
			if (TS_KEYWORDS.has(ident)) return wrap("hl-kw", ident);
			if (TS_TYPES.has(ident)) return wrap("hl-type", ident);
		}
		return match;
	});
}

// ---------------------------------------------------------------------------
// PHP — single-pass
// ---------------------------------------------------------------------------

const PHP_KEYWORDS = new Set([
	"declare", "namespace", "use", "final", "readonly", "class", "function",
	"public", "private", "protected", "static", "return", "new", "self",
	"array", "int", "float", "bool", "string", "mixed", "null", "true",
	"false", "strict_types",
]);

const PHP_RE = new RegExp([
	/(\/\*\*[\s\S]*?\*\/)/.source,              // doc comment (multiline)
	/(\/\/.*)/.source,                           // single-line comment
	/('(?:\\'|[^'])*')/.source,                  // single-quoted string
	/(\$\w+)/.source,                            // variable
	/(&lt;\?php)/.source,                        // opening tag (escaped)
	/(\b\d+\b)/.source,                          // number
	/(\b[A-Za-z_]\w*\b)/.source,                // identifier
].join("|"), "g");

export function highlightPhp(code: string): string {
	const escaped = escapeHtml(code);
	return escaped.replace(PHP_RE, (match, doc, comment, str, variable, phptag, num, ident) => {
		if (doc) return wrap("hl-comment", doc);
		if (comment) return wrap("hl-comment", comment);
		if (str) return wrap("hl-str", str);
		if (variable) return wrap("hl-var", variable);
		if (phptag) return wrap("hl-kw", phptag);
		if (num) return wrap("hl-num", num);
		if (ident) {
			if (PHP_KEYWORDS.has(ident)) return wrap("hl-kw", ident);
		}
		return match;
	});
}

// ---------------------------------------------------------------------------
// Python — single-pass
// ---------------------------------------------------------------------------

const PY_KEYWORDS = new Set([
	"from", "import", "class", "def", "return", "if", "elif", "else", "for",
	"in", "is", "not", "and", "or", "with", "as", "pass", "None", "True",
	"False", "self", "cls",
]);
const PY_TYPES = new Set([
	"str", "int", "float", "bool", "list", "dict", "tuple", "set",
	"Any", "Literal", "Optional", "Union",
]);

const PY_RE = new RegExp([
	/(#.*)/.source,                              // comment
	/^(@\w+(?:\(.*?\))?)/.source,               // decorator (start of line)
	/('(?:\\'|[^'])*')/.source,                  // single-quoted string
	/("(?:\\"|[^"])*")/.source,                  // double-quoted string (pre-escape)
	/(&quot;(?:[^&]|&(?!quot;))*?&quot;)/.source, // double-quoted string (post-escape)
	/(\b\d+(?:\.\d+)?\b)/.source,               // number
	/(\b[A-Za-z_]\w*\b)/.source,                // identifier
].join("|"), "gm");

export function highlightPython(code: string): string {
	const escaped = escapeHtml(code);
	return escaped.replace(PY_RE, (match, comment, decorator, sstr, dstr, estr, num, ident) => {
		if (comment) return wrap("hl-comment", comment);
		if (decorator) return wrap("hl-comment", decorator);
		if (sstr) return wrap("hl-str", sstr);
		if (dstr) return wrap("hl-str", dstr);
		if (estr) return wrap("hl-str", estr);
		if (num) return wrap("hl-num", num);
		if (ident) {
			if (PY_KEYWORDS.has(ident)) return wrap("hl-kw", ident);
			if (PY_TYPES.has(ident)) return wrap("hl-type", ident);
		}
		return match;
	});
}

// ---------------------------------------------------------------------------
// Java — single-pass
// ---------------------------------------------------------------------------

const JAVA_KEYWORDS = new Set([
	"package", "import", "public", "private", "protected", "static", "final",
	"record", "class", "interface", "enum", "extends", "implements", "new",
	"return", "var", "void", "if", "else", "for", "while", "switch", "case",
	"default", "throws", "throw", "try", "catch", "finally",
]);
const JAVA_TYPES = new Set([
	"String", "Integer", "Double", "Boolean", "Object", "Map", "List", "Set",
	"LinkedHashMap", "HashMap", "ArrayList",
	"int", "double", "boolean", "long", "float", "char", "byte", "short",
]);

const JAVA_RE = new RegExp([
	/(\/\/.*)/.source,                           // single-line comment
	/(@\w+)/.source,                             // annotation
	/("(?:\\"|[^"])*")/.source,                  // string (pre-escape)
	/(&quot;(?:[^&]|&(?!quot;))*?&quot;)/.source, // string (post-escape)
	/(\b\d+(?:\.\d+)?[fFdDlL]?\b)/.source,      // number
	/(\b[A-Za-z_]\w*\b)/.source,                // identifier
].join("|"), "g");

export function highlightJava(code: string): string {
	const escaped = escapeHtml(code);
	return escaped.replace(JAVA_RE, (match, comment, annotation, dstr, estr, num, ident) => {
		if (comment) return wrap("hl-comment", comment);
		if (annotation) return wrap("hl-comment", annotation);
		if (dstr) return wrap("hl-str", dstr);
		if (estr) return wrap("hl-str", estr);
		if (num) return wrap("hl-num", num);
		if (ident) {
			if (JAVA_KEYWORDS.has(ident)) return wrap("hl-kw", ident);
			if (JAVA_TYPES.has(ident)) return wrap("hl-type", ident);
		}
		return match;
	});
}
