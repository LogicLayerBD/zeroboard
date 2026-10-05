// Minimal Markdown renderer for card descriptions. All input is HTML-escaped
// before any formatting is applied, so user text can never inject markup; the
// only tags emitted are the fixed ones below, and links are limited to safe schemes.

const SAFE_LINK = /^(https?:\/\/|mailto:)/i;
const HEADING_TAGS: Record<number, string> = {
	1: '<h3 class="mt-3 mb-1 text-lg font-semibold">',
	2: '<h4 class="mt-3 mb-1 text-base font-semibold">',
	3: '<h5 class="mt-2 mb-1 text-sm font-semibold">'
};
const HEADING_CLOSE: Record<number, string> = { 1: '</h3>', 2: '</h4>', 3: '</h5>' };

function escapeHtml(text: string): string {
	return text
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;')
		.replace(/"/g, '&quot;')
		.replace(/'/g, '&#39;');
}

/** Inline formatting on already-escaped text. */
function renderInline(escaped: string): string {
	const codeSpans: string[] = [];
	let html = escaped.replace(/`([^`]+)`/g, (_, code: string) => {
		codeSpans.push(`<code class="rounded bg-slate-100 px-1 text-sm">${code}</code>`);
		return `\u0000${codeSpans.length - 1}\u0000`;
	});
	html = html
		.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (match, label: string, href: string) => {
			// `href` is escaped text; unescape only to test the scheme.
			const raw = href.replace(/&amp;/g, '&');
			if (!SAFE_LINK.test(raw)) return match;
			return `<a href="${href}" target="_blank" rel="noopener noreferrer nofollow" class="text-indigo-600 underline">${label}</a>`;
		})
		.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
		.replace(/(^|[^*])\*([^*]+)\*/g, '$1<em>$2</em>');
	return html.replace(/\u0000(\d+)\u0000/g, (_, index: string) => codeSpans[Number(index)] ?? '');
}

export function renderMarkdown(source: string): string {
	const lines = escapeHtml(source.replace(/\r\n?/g, '\n')).split('\n');
	const out: string[] = [];
	let paragraph: string[] = [];
	let listTag: 'ul' | 'ol' | null = null;
	let inCode = false;
	let code: string[] = [];

	const flushParagraph = () => {
		if (paragraph.length > 0) {
			out.push(`<p class="my-2">${paragraph.map(renderInline).join('<br>')}</p>`);
			paragraph = [];
		}
	};
	const closeList = () => {
		if (listTag) {
			out.push(`</${listTag}>`);
			listTag = null;
		}
	};
	const openList = (tag: 'ul' | 'ol') => {
		if (listTag === tag) return;
		closeList();
		const style = tag === 'ul' ? 'list-disc' : 'list-decimal';
		out.push(`<${tag} class="my-2 ml-5 ${style}">`);
		listTag = tag;
	};

	for (const line of lines) {
		if (line.trimStart().startsWith('```')) {
			if (inCode) {
				out.push(
					`<pre class="my-2 overflow-x-auto rounded bg-slate-100 p-2 text-sm"><code>${code.join('\n')}</code></pre>`
				);
				code = [];
				inCode = false;
			} else {
				flushParagraph();
				closeList();
				inCode = true;
			}
			continue;
		}
		if (inCode) {
			code.push(line);
			continue;
		}

		const heading = /^(#{1,3})\s+(.*)$/.exec(line);
		const bullet = /^\s*[-*]\s+(.*)$/.exec(line);
		const numbered = /^\s*\d+\.\s+(.*)$/.exec(line);
		if (heading) {
			flushParagraph();
			closeList();
			const level = heading[1].length;
			out.push(`${HEADING_TAGS[level]}${renderInline(heading[2])}${HEADING_CLOSE[level]}`);
		} else if (bullet || numbered) {
			flushParagraph();
			openList(bullet ? 'ul' : 'ol');
			out.push(`<li>${renderInline((bullet ?? numbered)?.[1] ?? '')}</li>`);
		} else if (line.trim() === '') {
			flushParagraph();
			closeList();
		} else {
			closeList();
			paragraph.push(line);
		}
	}
	if (inCode) {
		out.push(
			`<pre class="my-2 overflow-x-auto rounded bg-slate-100 p-2 text-sm"><code>${code.join('\n')}</code></pre>`
		);
	}
	flushParagraph();
	closeList();
	return out.join('');
}
