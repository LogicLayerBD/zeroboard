const MINUTES_PER_HOUR = 60;
const BYTES_PER_KIB = 1024;
const BYTE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];
const MAX_INITIALS = 2;
const DATE_INPUT_PAD = 2;

export function formatDate(ms: number): string {
	return new Date(ms).toLocaleDateString(undefined, {
		month: 'short',
		day: 'numeric',
		year: 'numeric'
	});
}

export function formatDateTime(ms: number): string {
	return new Date(ms).toLocaleString(undefined, {
		month: 'short',
		day: 'numeric',
		year: 'numeric',
		hour: 'numeric',
		minute: '2-digit'
	});
}

/** Value for `<input type="datetime-local">` in the browser's time zone. */
export function toDateTimeLocal(ms: number | null): string {
	if (ms === null) return '';
	const d = new Date(ms);
	const pad = (n: number) => String(n).padStart(DATE_INPUT_PAD, '0');
	return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** Parses a `datetime-local` value as local time; empty or invalid input is `null`. */
export function fromDateTimeLocal(value: string): number | null {
	if (!value) return null;
	const ms = new Date(value).getTime();
	return Number.isNaN(ms) ? null : ms;
}

export function isOverdue(dueDate: number | null): boolean {
	return dueDate !== null && dueDate < Date.now();
}

export function formatMinutes(total: number): string {
	const hours = Math.floor(total / MINUTES_PER_HOUR);
	const minutes = total % MINUTES_PER_HOUR;
	if (hours === 0) return `${minutes}m`;
	return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}

export function formatBytes(bytes: number): string {
	let value = bytes;
	let unit = 0;
	while (value >= BYTES_PER_KIB && unit < BYTE_UNITS.length - 1) {
		value /= BYTES_PER_KIB;
		unit += 1;
	}
	return unit === 0 ? `${value} ${BYTE_UNITS[unit]}` : `${value.toFixed(1)} ${BYTE_UNITS[unit]}`;
}

export function initials(name: string): string {
	return name
		.trim()
		.split(/\s+/)
		.slice(0, MAX_INITIALS)
		.map((part) => part.charAt(0).toUpperCase())
		.join('');
}
