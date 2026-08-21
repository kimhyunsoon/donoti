import { svg, type TemplateResult } from 'lit';

// Lucide 스타일 스트로크 아이콘 (24x24, currentColor)
const PATHS: Record<string, TemplateResult> = {
  settings: svg`<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>`,
  bell: svg`<path d="M10.268 21a2 2 0 0 0 3.464 0"/><path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"/>`,
  'chevron-left': svg`<path d="m15 18-6-6 6-6"/>`,
  'chevron-right': svg`<path d="m9 18 6-6-6-6"/>`,
  'log-out': svg`<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>`,
  'bell-off': svg`<path d="M8.7 3A6 6 0 0 1 18 8c0 4.499 1.411 5.956 2.738 7.326A1 1 0 0 1 20 17h-3"/><path d="M10.268 21a2 2 0 0 0 3.464 0"/><path d="m2 2 20 20"/><path d="M6.26 6.26A5.86 5.86 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326A1 1 0 0 0 4 17h9"/>`,
  refresh: svg`<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>`,
};

/** 아이콘 렌더링. size 기본 20px, 스트로크는 currentColor */
export function icon(name: keyof typeof PATHS | string, size = 20): TemplateResult {
  return svg`<svg xmlns="http://www.w3.org/2000/svg" width=${size} height=${size} viewBox="0 0 24 24"
    fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"
    style="display:block">${PATHS[name] ?? ''}</svg>`;
}
