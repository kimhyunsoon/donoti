export type ThemeMode = 'auto' | 'light' | 'dark';

export function currentTheme(): ThemeMode {
  const saved = localStorage.getItem('theme');
  return saved === 'light' || saved === 'dark' ? saved : 'auto';
}

// data-theme 속성으로 index.html의 토큰 오버라이드를 전환
export function applyTheme(mode: ThemeMode): void {
  if (mode === 'auto') {
    delete document.documentElement.dataset.theme;
    localStorage.removeItem('theme');
  } else {
    document.documentElement.dataset.theme = mode;
    localStorage.setItem('theme', mode);
  }
}
