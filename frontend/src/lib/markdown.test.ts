import { describe, expect, it } from 'vitest';
import { markdownToPlainText, renderMarkdown } from './markdown';

describe('markdownToPlainText', () => {
	it('strips headings, list markers and inline formatting', () => {
		const source = '## Goal\n\nShip a **polished** *first* version.\n\n- Wireframes\n1. Review `api`';
		expect(markdownToPlainText(source)).toBe(
			'Goal\nShip a polished first version.\nWireframes\nReview api'
		);
	});

	it('keeps link labels and drops URLs', () => {
		expect(markdownToPlainText('See [the spec](https://example.com/spec).')).toBe('See the spec.');
	});

	it('drops code fence markers but keeps the code text', () => {
		expect(markdownToPlainText('```\nnpm run build\n```')).toBe('npm run build');
	});

	it('leaves HTML-like text untouched because the result is rendered as text', () => {
		expect(markdownToPlainText('<img src=x onerror=alert(1)>')).toBe('<img src=x onerror=alert(1)>');
	});
});

describe('renderMarkdown', () => {
	it('escapes raw HTML', () => {
		expect(renderMarkdown('<script>alert(1)</script>')).not.toContain('<script>');
	});

	it('refuses javascript: links', () => {
		expect(renderMarkdown('[x](javascript:alert(1))')).not.toContain('href=');
	});
});
