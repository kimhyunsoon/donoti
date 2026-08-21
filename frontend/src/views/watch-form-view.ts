import { LitElement, html, css, nothing, type TemplateResult } from 'lit';
import { customElement, state } from 'lit/decorators.js';
import { tokens, ui } from '../style.js';
import { api } from '../api.js';
import { icon } from '../icons.js';
import { toast } from '../ui.js';
import { CATALOG, FX_CURRENCIES, STOCK_KIND_LABELS, findProvider, findProviderById, providerAvatar, type ProviderInfo } from '../catalog.js';
import { DAY_LABELS, defaultRule, type ScheduleRule } from '../schedule.js';
import '../components/schedule-editor.js';
import '../sheets/calendar-sheet.js';
import '../sheets/time-wheel-sheet.js';
import type { CalendarSheet } from '../sheets/calendar-sheet.js';
import type { TimeWheelSheet } from '../sheets/time-wheel-sheet.js';

// Date → 'YYYY-MM-DD'
function toYmd(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

interface WatchRow {
  id: number;
  category: string;
  provider: string;
  name: string;
  schedule: string;
  config: string;
  ends_at: string | null;
}

interface StockSymbol {
  code: string;
  name: string;
  kind: string;
}

interface EmartStore {
  code: string;
  name: string;
  area: string;
}

// 이마트 공유 링크 해석 결과 (backend /api/emart/resolve)
interface EmartResolved {
  type: 'digital' | 'product';
  sku: string;
  name: string;
  price: number;
  link: string;
  linkedStoreCode: string;
  stores: EmartStore[];
}

const EMART_TYPE_LABELS: Record<string, string> = { digital: '픽업', product: '매장' };

interface CgvMovie {
  code: string;
  name: string;
}

interface CgvTheater {
  code: string;
  name: string;
  area: string;
}

// 영화 폼(영화·지점·날짜·시간범위)을 공유하는 provider들
const MOVIE_PROVIDERS = ['cgv', 'lotte', 'megabox'];

// 알림 등록·수정. 단계마다 라우트가 분리된다:
//   #/watch/new              종류 선택 (2뎁스 픽커)
//   #/watch/new/<provider>   선택한 종류의 공통 등록 폼
//   #/watch/edit?id=N        수정 폼
@customElement('watch-form-view')
export class WatchFormView extends LitElement {
  static styles = [
    tokens,
    ui,
    css`
      :host {
        display: block;
        max-width: 480px;
        margin: 0 auto;
        min-height: 100dvh;
        padding-bottom: calc(24px + env(safe-area-inset-bottom));
      }
      .block { margin: 12px 16px; }
      .section-label {
        margin: 18px 20px 8px;
        font-size: 0.82rem;
        font-weight: 700;
        color: var(--text-sub);
      }
      .pick-row {
        display: flex;
        align-items: center;
        gap: 12px;
        width: 100%;
        padding: 10px 0;
        background: none;
        text-align: left;
        font-size: 0.96rem;
        font-weight: 600;
        color: var(--text);
      }
      .pick-row .chev { margin-left: auto; color: var(--text-sub); }
      .target { display: flex; align-items: center; gap: 12px; }
      .target .names { display: flex; flex-direction: column; gap: 1px; }
      .target .names b { font-size: 0.98rem; }
      .target .change { margin-left: auto; }
      .ends-body { margin-top: 4px; border-top: 1px solid var(--border); padding-top: 4px; }
      /* 날짜·시각 선택 버튼 - 탭하면 하단 시트 */
      .pick-btn {
        padding: 8px 14px;
        border-radius: 10px;
        background: var(--bg);
        font-weight: 600;
        font-size: 0.92rem;
        font-variant-numeric: tabular-nums;
        color: var(--text);
      }
      .pick-btn:active { background: var(--surface-weak); }
      .danger-btn { color: var(--danger); width: 100%; padding: 13px; font-size: 0.92rem; }
      /* provider별 설정 - 선택 칩 */
      .chips { display: flex; flex-wrap: wrap; gap: 8px; }
      .chip {
        padding: 9px 14px;
        border-radius: 12px;
        background: var(--bg);
        color: var(--text-sub);
        font-size: 0.9rem;
        font-weight: 600;
      }
      .chip.on { background: var(--accent); color: #fff; font-weight: 700; }
      /* 종목 검색 결과 */
      .result-row {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 10px 4px;
        background: none;
        text-align: left;
        color: var(--text);
        border-bottom: 1px solid var(--border);
      }
      .result-row:last-child { border-bottom: none; }
      .result-row b { font-size: 0.92rem; font-weight: 600; }
      .result-row .code { color: var(--text-sub); font-size: 0.8rem; }
      .kind-badge {
        margin-left: auto;
        flex-shrink: 0;
        padding: 3px 8px;
        border-radius: 7px;
        background: var(--bg);
        color: var(--text-sub);
        font-size: 0.74rem;
        font-weight: 700;
      }
      .stock-sel { display: flex; align-items: center; gap: 10px; }
      .stock-sel .names { display: flex; flex-direction: column; gap: 1px; }
      .stock-sel .names b { font-size: 0.96rem; }
      .stock-sel button { margin-left: auto; }
      /* 이마트 링크 입력 + 불러오기 버튼 */
      .link-line { display: flex; gap: 8px; align-items: stretch; }
      .link-line input { flex: 1; min-width: 0; }
      .link-go {
        width: 52px;
        border-radius: 14px;
        background: var(--accent);
        color: #fff;
        display: grid;
        place-items: center;
        flex-shrink: 0;
        transition: transform 0.08s;
      }
      .link-go:active { transform: scale(0.96); }
      .link-go:disabled { opacity: 0.35; }
      /* 선택된 지점 칩 */
      .store-chips { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 10px; }
      .store-chip {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 7px 8px 7px 12px;
        border-radius: 10px;
        background: var(--accent-soft);
        color: var(--accent);
        font-size: 0.85rem;
        font-weight: 700;
      }
      .store-chip button {
        background: none;
        color: inherit;
        display: flex;
        padding: 2px;
        border-radius: 50%;
      }
      /* 이마트 감시 지점 선택 리스트 */
      .store-list {
        max-height: 264px;
        overflow-y: auto;
        background: var(--bg);
        border-radius: 12px;
        padding: 4px 12px;
      }
      .store-row {
        display: flex;
        align-items: center;
        gap: 10px;
        width: 100%;
        padding: 10px 2px;
        background: none;
        text-align: left;
        color: var(--text);
        border-bottom: 1px solid var(--border);
      }
      .store-row:last-child { border-bottom: none; }
      .store-row .names { display: flex; align-items: baseline; gap: 7px; min-width: 0; }
      .store-row b { font-size: 0.92rem; font-weight: 600; }
    `,
  ];

  @state() private editId: number | null = null;
  @state() private categoryId = '';
  @state() private providerId = '';
  @state() private config: Record<string, unknown> = {};
  @state() private rules: ScheduleRule[] = [defaultRule()];
  // 종목 검색 상태
  @state() private stockQuery = '';
  @state() private stockResults: StockSymbol[] = [];
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  // 이마트 링크 해석 상태
  @state() private emartLink = '';
  @state() private emartInfo: EmartResolved | null = null;
  @state() private emartLoading = false;
  @state() private emartStoreQuery = '';
  // CGV 목록 상태
  @state() private cgvMovies: CgvMovie[] | null = null;
  @state() private cgvTheaters: CgvTheater[] | null = null;
  @state() private cgvMovieQuery = '';
  @state() private cgvTheaterQuery = '';
  @state() private endsEnabled = false;
  @state() private endsDate = '';
  @state() private endsHour = '18';
  @state() private saving = false;

  connectedCallback(): void {
    super.connectedCallback();
    const [path = '', query] = location.hash.split('?');
    if (path === '#/watch/edit') {
      const id = Number(new URLSearchParams(query).get('id'));
      this.editId = id;
      void this.load(id);
      return;
    }
    if (path.startsWith('#/watch/new/')) {
      const info = findProviderById(path.slice('#/watch/new/'.length));
      if (!info) {
        location.hash = '#/watch/new';
        return;
      }
      this.categoryId = info.category.id;
      this.providerId = info.provider.id;
      if (MOVIE_PROVIDERS.includes(this.providerId)) void this.loadCgvData();
    }
  }

  // 영화 provider의 상영작·지점 목록 로드 (/api/<provider>/movies|theaters)
  private async loadCgvData(): Promise<void> {
    void api<CgvMovie[]>(`/api/${this.providerId}/movies`)
      .then((movies) => { this.cgvMovies = movies; })
      .catch(() => toast('상영작을 불러오지 못했어요'));
    void api<CgvTheater[]>(`/api/${this.providerId}/theaters`)
      .then((theaters) => { this.cgvTheaters = theaters; })
      .catch(() => {});
  }

  private async load(id: number): Promise<void> {
    const row = await api<WatchRow>(`/api/watches/${id}`);
    this.categoryId = row.category;
    this.providerId = row.provider;
    this.config = JSON.parse(row.config) as Record<string, unknown>;
    this.rules = JSON.parse(row.schedule) as ScheduleRule[];
    if (MOVIE_PROVIDERS.includes(row.provider)) void this.loadCgvData();
    // 이마트 수정: 저장된 링크를 다시 해석해 지점 목록을 되살린다 (실패해도 기존 설정으로 표시)
    if (row.provider === 'emart' && typeof this.config.link === 'string') {
      void api<EmartResolved>('/api/emart/resolve', {
        method: 'POST',
        body: JSON.stringify({ url: this.config.link }),
      })
        .then((info) => { this.emartInfo = info; })
        .catch(() => {});
    }
    if (row.ends_at) {
      this.endsEnabled = true;
      this.endsDate = row.ends_at.slice(0, 10);
      this.endsHour = row.ends_at.slice(11, 13);
    }
  }

  // 픽커 단계인지 (new 라우트에서 provider 미선택)
  private get picking(): boolean {
    return this.editId === null && this.providerId === '';
  }

  private get provider(): ProviderInfo | null {
    return findProvider(this.categoryId, this.providerId)?.provider ?? null;
  }

  private back(): void {
    if (this.editId === null && !this.picking) {
      location.hash = '#/watch/new';
      return;
    }
    location.hash = '#/home';
  }

  // provider 설정으로부터 자동 이름 계산 ('달러 환율', '삼성전자 주가', '코스피 지수')
  private autoNameFromConfig(provider: string, config: Record<string, unknown>): string {
    if (provider === 'fx') {
      const cur = FX_CURRENCIES.find((c) => c.code === config.currency);
      return cur ? `${cur.label} 환율` : '';
    }
    if (provider === 'stock' && typeof config.name === 'string') {
      return config.kind === 'index' ? `${config.name} 지수` : `${config.name} 주가`;
    }
    if (provider === 'emart' && typeof config.name === 'string') return config.name;
    if (MOVIE_PROVIDERS.includes(provider) && typeof config.movieName === 'string') return config.movieName;
    return '';
  }

  private applyConfig(config: Record<string, unknown>): void {
    this.config = config;
  }

  // 종료 시점 켜기 - 기본값은 지금부터 1일 뒤
  private onEndsToggle(e: Event): void {
    const on = (e.target as HTMLInputElement).checked;
    this.endsEnabled = on;
    if (on && this.endsDate === '') {
      this.endsDate = toYmd(new Date(Date.now() + 86_400_000));
      this.endsHour = String(new Date().getHours()).padStart(2, '0');
    }
  }

  // '8월 22일 (토)', 다른 해면 연도 포함
  private dateLabel(value: string): string {
    const d = new Date(`${value}T00:00`);
    const label = `${d.getMonth() + 1}월 ${d.getDate()}일 (${DAY_LABELS[d.getDay()]})`;
    return d.getFullYear() === new Date().getFullYear() ? label : `${d.getFullYear()}년 ${label}`;
  }

  private async pickEndsDate(): Promise<void> {
    const cal = this.renderRoot.querySelector('calendar-sheet') as CalendarSheet;
    const picked = await cal.show(this.endsDate, { title: '종료 날짜', min: toYmd(new Date()) });
    if (picked !== null) this.endsDate = picked;
  }

  private async pickEndsHour(): Promise<void> {
    const wheel = this.renderRoot.querySelector('time-wheel-sheet') as TimeWheelSheet;
    const picked = await wheel.show(`${this.endsHour}:00`, { title: '종료 시각', hourOnly: true });
    if (picked !== null) this.endsHour = picked.slice(0, 2);
  }

  // 이마트 공유 링크 해석 - 성공 시 상품·지점 목록 확보, 링크 지점을 기본 선택
  private async resolveEmart(): Promise<void> {
    if (this.emartLink.trim() === '' || this.emartLoading) return;
    this.emartLoading = true;
    try {
      const info = await api<EmartResolved>('/api/emart/resolve', {
        method: 'POST',
        body: JSON.stringify({ url: this.emartLink }),
      });
      this.emartInfo = info;
      const linked = info.stores.find((s) => s.code === info.linkedStoreCode);
      this.applyConfig({
        ...this.config,
        type: info.type,
        sku: info.sku,
        name: info.name,
        link: info.link,
        stores: linked ? [{ code: linked.code, name: linked.name }] : [],
      });
    } catch (err) {
      const message = err instanceof Error && !err.message.startsWith('api_error') ? err.message : '';
      toast(message || '링크를 불러오지 못했어요');
    } finally {
      this.emartLoading = false;
    }
  }

  // 감시 지점 선택 토글
  private toggleEmartStore(store: EmartStore): void {
    const stores = (this.config.stores ?? []) as { code: string; name: string }[];
    const next = stores.some((s) => s.code === store.code)
      ? stores.filter((s) => s.code !== store.code)
      : [...stores, { code: store.code, name: store.name }];
    this.applyConfig({ ...this.config, stores: next });
  }

  // 이마트 링크·상품 선택 초기화 (알림 옵션은 유지)
  private resetEmart(): void {
    this.emartInfo = null;
    this.emartLink = '';
    this.emartStoreQuery = '';
    this.config = { endOnStock: this.config.endOnStock, onlyInStock: this.config.onlyInStock };
  }

  // 종목 검색 (250ms 디바운스)
  private onStockQuery(e: Event): void {
    this.stockQuery = (e.target as HTMLInputElement).value;
    if (this.searchTimer) clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.searchStocks(), 250);
  }

  private async searchStocks(): Promise<void> {
    const q = this.stockQuery.trim();
    if (q === '') {
      this.stockResults = [];
      return;
    }
    this.stockResults = await api<StockSymbol[]>(`/api/stocks?q=${encodeURIComponent(q)}`);
  }

  // 폼 → API 페이로드 검증·변환. 문제가 있으면 toast 후 null
  private buildPayload(): {
    name: string;
    schedule: ScheduleRule[];
    config: Record<string, unknown>;
    endsAt: string | null;
  } | null {
    if (this.providerId === 'fx' && typeof this.config.currency !== 'string') {
      toast('통화를 선택해 주세요');
      return null;
    }
    if (this.providerId === 'stock' && typeof this.config.code !== 'string') {
      toast('종목을 선택해 주세요');
      return null;
    }
    if (this.providerId === 'emart') {
      if (typeof this.config.sku !== 'string') {
        toast('이마트 공유 링크를 불러와 주세요');
        return null;
      }
      if (!Array.isArray(this.config.stores) || this.config.stores.length === 0) {
        toast('지점을 1곳 이상 선택해 주세요');
        return null;
      }
    }
    if (MOVIE_PROVIDERS.includes(this.providerId)) {
      if (typeof this.config.movieNo !== 'string') {
        toast('영화를 선택해 주세요');
        return null;
      }
      if (!Array.isArray(this.config.stores) || this.config.stores.length === 0) {
        toast('지점을 1곳 이상 선택해 주세요');
        return null;
      }
      const dates = this.config.dates as string[] | undefined;
      if (!Array.isArray(dates) || dates.length === 0) {
        toast('날짜를 1개 이상 선택해 주세요');
        return null;
      }
      const start = (this.config.start as string | undefined) ?? '00:00';
      const end = (this.config.end as string | undefined) ?? '23:59';
      if (start > end) {
        toast('시작 시각이 종료 시각보다 늦어요');
        return null;
      }
      this.config = { ...this.config, start, end };
    }
    if (this.rules.some((r) => r.days.length === 0)) {
      toast('요일을 1개 이상 선택해 주세요');
      return null;
    }
    if (this.rules.some((r) => r.mode === 'interval' && r.start! > r.end!)) {
      toast('시작 시각이 종료 시각보다 늦어요');
      return null;
    }
    if (this.endsEnabled && !this.endsDate) {
      toast('종료 날짜를 선택해 주세요');
      return null;
    }
    // 영화는 검토 기간이 끝나면 자동 종료 - 마지막 선택 날짜의 종료 시각 (분까지)
    const endsAt =
      MOVIE_PROVIDERS.includes(this.providerId)
        ? `${[...(this.config.dates as string[])].sort().at(-1) ?? ''} ${this.config.end as string}`
        : this.endsEnabled
          ? `${this.endsDate} ${this.endsHour}:00`
          : null;
    return {
      // 이름은 항상 설정 기반 자동 생성
      name: this.autoNameFromConfig(this.providerId, this.config) || `${this.provider?.label ?? ''} 알림`,
      schedule: this.rules,
      config: this.config,
      endsAt,
    };
  }

  private async save(): Promise<void> {
    const payload = this.buildPayload();
    if (!payload) return;
    this.saving = true;
    try {
      if (this.editId === null) {
        await api('/api/watches', {
          method: 'POST',
          body: JSON.stringify({ category: this.categoryId, provider: this.providerId, ...payload }),
        });
        toast('알림을 만들었어요');
      } else {
        await api(`/api/watches/${this.editId}`, { method: 'PUT', body: JSON.stringify(payload) });
        toast('알림을 수정했어요');
      }
      location.hash = '#/home';
    } catch (err) {
      // 서버 검증 메시지(한국어)는 그대로 노출
      const message = err instanceof Error && !err.message.startsWith('api_error') ? err.message : '';
      toast(message || '저장하지 못했어요. 다시 시도해 주세요');
    } finally {
      this.saving = false;
    }
  }

  // HTMLElement.remove()와 이름이 겹치지 않도록 removeWatch
  private async removeWatch(): Promise<void> {
    if (this.editId === null) return;
    await api(`/api/watches/${this.editId}`, { method: 'DELETE' });
    toast('삭제했어요. 종료된 알림에서 복구할 수 있어요');
    location.hash = '#/home';
  }

  private renderPick(): TemplateResult {
    return html`
      ${CATALOG.map(
        (category) => html`
          <div class="section-label">${category.label}</div>
          <div class="block card" style="margin-top:0;padding:8px 20px">
            ${category.providers.map(
              (p) => html`
                <button class="pick-row" @click=${(): void => { location.hash = `#/watch/new/${p.id}`; }}>
                  ${providerAvatar(p, 38)}
                  <span>${p.label}</span>
                  <span class="chev">${icon('chevron-right', 18)}</span>
                </button>
              `,
            )}
          </div>
        `,
      )}
    `;
  }

  // provider별 상세 설정 폼 (미구현 provider는 안내만)
  private renderProviderConfig(): TemplateResult {
    if (this.providerId === 'fx') {
      return html`
        <div class="chips">
          ${FX_CURRENCIES.map(
            (c) => html`
              <button class="chip ${this.config.currency === c.code ? 'on' : ''}"
                @click=${(): void => this.applyConfig({ ...this.config, currency: c.code })}>${c.label}</button>
            `,
          )}
        </div>
      `;
    }
    if (this.providerId === 'stock') return this.renderStockConfig();
    if (this.providerId === 'emart') return this.renderEmartConfig();
    if (MOVIE_PROVIDERS.includes(this.providerId)) return this.renderCgvConfig();
    return html`<p class="sub" style="margin:0">상세 설정은 준비 중이에요. 다음 업데이트에서 제공돼요.</p>`;
  }

  // CGV 시간 범위 선택 (상영 시작시간 기준)
  private async pickCgvTime(which: 'start' | 'end'): Promise<void> {
    const wheel = this.renderRoot.querySelector('time-wheel-sheet') as TimeWheelSheet;
    const current = (this.config[which] as string | undefined) ?? (which === 'start' ? '00:00' : '23:59');
    const picked = await wheel.show(current, { title: which === 'start' ? '시작 시각' : '종료 시각' });
    if (picked !== null) this.applyConfig({ ...this.config, [which]: picked });
  }

  // 상영 날짜 여러 개 선택 (캘린더 시트)
  private async pickMovieDates(): Promise<void> {
    const cal = this.renderRoot.querySelector('calendar-sheet') as CalendarSheet;
    const picked = await cal.showMulti((this.config.dates ?? []) as string[], {
      title: '상영 날짜',
      min: toYmd(new Date()),
    });
    if (picked !== null) this.applyConfig({ ...this.config, dates: picked });
  }

  private removeMovieDate(date: string): void {
    const dates = (this.config.dates ?? []) as string[];
    this.applyConfig({ ...this.config, dates: dates.filter((d) => d !== date) });
  }

  // CGV - 영화 → 지점 → 날짜(여러 개) → 시간 범위 → 예매 열림 알림
  private renderCgvConfig(): TemplateResult {
    return html`
      ${this.renderCgvMovie()}
      ${typeof this.config.movieNo === 'string' ? this.renderCgvDetail() : nothing}
    `;
  }

  private renderCgvMovie(): TemplateResult {
    if (typeof this.config.movieNo === 'string') {
      return html`
        <div class="stock-sel" style="margin-bottom:14px">
          <span class="names"><b>${this.config.movieName}</b></span>
          <button class="btn-ghost" @click=${(): void => {
            this.config = { start: this.config.start, end: this.config.end, imaxOnly: this.config.imaxOnly };
            this.cgvMovieQuery = '';
          }}>변경</button>
        </div>
      `;
    }
    const query = this.cgvMovieQuery.trim();
    const movies = (this.cgvMovies ?? []).filter((m) => query === '' || m.name.includes(query));
    return html`
      <input
        .value=${this.cgvMovieQuery}
        placeholder="영화 검색"
        style="background:var(--bg);padding:10px 14px;margin-bottom:8px"
        @input=${(e: Event): void => { this.cgvMovieQuery = (e.target as HTMLInputElement).value; }}
      >
      ${this.cgvMovies === null
        ? html`<p class="sub" style="margin:4px 0 0">상영작을 불러오는 중...</p>`
        : movies.length === 0
          ? html`<p class="sub" style="margin:4px 0 0">검색 결과가 없어요</p>`
          : html`
              <div class="store-list">
                ${movies.map(
                  (m) => html`
                    <button class="store-row" @click=${(): void => {
                      this.applyConfig({ ...this.config, movieNo: m.code, movieName: m.name });
                    }}>
                      <span class="names"><b>${m.name}</b></span>
                    </button>
                  `,
                )}
              </div>
            `}
    `;
  }

  private renderCgvDetail(): TemplateResult {
    const selected = (this.config.stores ?? []) as { code: string; name: string }[];
    const theaterQuery = this.cgvTheaterQuery.trim();
    const allTheaters = this.cgvTheaters ?? [];
    const candidates = allTheaters.filter(
      (t) =>
        !selected.some((v) => v.code === t.code) &&
        (theaterQuery === '' || t.name.includes(theaterQuery) || t.area.includes(theaterQuery)),
    );
    const dates = (this.config.dates ?? []) as string[];
    const start = (this.config.start as string | undefined) ?? '00:00';
    const end = (this.config.end as string | undefined) ?? '23:59';
    return html`
      <div class="frow" style="min-height:auto;margin:14px 0 8px">
        <span class="lbl">지점</span>
        <span class="sub">${selected.length}곳 선택</span>
      </div>
      ${selected.length > 0
        ? html`
            <div class="store-chips">
              ${selected.map(
                (s) => html`
                  <span class="store-chip">
                    ${s.name}
                    <button aria-label="${s.name} 제거"
                      @click=${(): void => this.toggleEmartStore({ ...s, area: '' })}>
                      ${icon('x', 13)}
                    </button>
                  </span>
                `,
              )}
            </div>
          `
        : nothing}
      <input
        .value=${this.cgvTheaterQuery}
        placeholder="지점 검색"
        style="background:var(--bg);padding:10px 14px;margin-bottom:8px"
        @input=${(e: Event): void => { this.cgvTheaterQuery = (e.target as HTMLInputElement).value; }}
      >
      ${candidates.length > 0
        ? html`
            <div class="store-list" style="max-height:200px">
              ${candidates.map(
                (t) => html`
                  <button class="store-row" @click=${(): void => this.toggleEmartStore(t)}>
                    <span class="names">
                      <b>${t.name}</b>
                      <span class="sub">${t.area}</span>
                    </span>
                  </button>
                `,
              )}
            </div>
          `
        : html`<p class="sub" style="margin:4px 0 0">검색 결과가 없어요</p>`}

      <div class="frow" style="margin:6px 0 0">
        <span class="lbl">날짜</span>
        <span class="ctl">
          <button class="pick-btn" @click=${(): void => void this.pickMovieDates()}>
            ${dates.length > 0 ? `${dates.length}일 선택됨` : '날짜 선택'}
          </button>
        </span>
      </div>
      ${dates.length > 0
        ? html`
            <div class="store-chips" style="margin-top:4px">
              ${dates.map(
                (d) => html`
                  <span class="store-chip">
                    ${this.dateLabel(d)}
                    <button aria-label="${d} 제거" @click=${(): void => this.removeMovieDate(d)}>
                      ${icon('x', 13)}
                    </button>
                  </span>
                `,
              )}
            </div>
          `
        : nothing}

      <div class="frow" style="margin-top:4px">
        <span class="lbl">시간</span>
        <span class="ctl">
          <button class="pick-btn" @click=${(): void => void this.pickCgvTime('start')}>${start}</button>
          <span style="color:var(--text-sub)">~</span>
          <button class="pick-btn" @click=${(): void => void this.pickCgvTime('end')}>${end}</button>
        </span>
      </div>

      ${this.providerId === 'cgv'
        ? html`
            <div class="frow" style="min-height:auto;margin-top:10px">
              <span style="font-weight:700;font-size:0.92rem">IMAX만 알림</span>
              <label class="switch">
                <input type="checkbox" ?checked=${this.config.imaxOnly === true}
                  @change=${(e: Event): void => {
                    this.applyConfig({ ...this.config, imaxOnly: (e.target as HTMLInputElement).checked });
                  }}>
                <span class="knob"></span>
              </label>
            </div>
          `
        : nothing}
      <p class="sub" style="margin:6px 0 0">예매 가능한 회차가 있을 때만 알림이 와요</p>
    `;
  }

  // 이마트 - 공유 링크 입력 → 상품 확인 → 지점 선택 (선택된 지점은 칩으로)
  private renderEmartConfig(): TemplateResult {
    if (typeof this.config.sku !== 'string') {
      const validLink = /^https?:\/\/\S+/.test(this.emartLink.trim());
      return html`
        <div class="link-line">
          <input
            .value=${this.emartLink}
            placeholder="이마트 앱 공유 링크 붙여넣기"
            style="background:var(--bg)"
            @input=${(e: Event): void => { this.emartLink = (e.target as HTMLInputElement).value; }}
            @keydown=${(e: KeyboardEvent): void => { if (e.key === 'Enter') void this.resolveEmart(); }}
          >
          <button class="link-go" aria-label="상품 불러오기"
            ?disabled=${!validLink || this.emartLoading}
            @click=${(): void => void this.resolveEmart()}>
            ${icon('arrow-right', 19)}
          </button>
        </div>
        <p class="sub" style="margin:8px 0 0">상품 페이지의 공유하기로 복사한 링크를 넣어 주세요.</p>
      `;
    }

    const selected = (this.config.stores ?? []) as { code: string; name: string }[];
    const query = this.emartStoreQuery.trim();
    const allStores = this.emartInfo?.stores ?? [];
    // 선택된 지점은 칩으로 올라가므로 목록에서 제외
    const candidates = allStores.filter((s) => !selected.some((v) => v.code === s.code));
    const stores = query === ''
      ? candidates
      : candidates.filter((s) => s.name.includes(query) || s.area.includes(query));
    return html`
      <div class="stock-sel" style="margin-bottom:14px">
        <span class="names">
          <b>${this.config.name}</b>
          <span class="sub">
            ${EMART_TYPE_LABELS[String(this.config.type)] ?? ''}
            ${this.emartInfo ? ` · ${this.emartInfo.price.toLocaleString('ko-KR')}원` : ''}
          </span>
        </span>
        <button class="btn-ghost" @click=${(): void => this.resetEmart()}>변경</button>
      </div>

      <div class="frow" style="min-height:auto;margin-bottom:8px">
        <span class="lbl">지점</span>
        <span class="sub">${selected.length}곳 선택</span>
      </div>
      ${selected.length > 0
        ? html`
            <div class="store-chips">
              ${selected.map(
                (s) => html`
                  <span class="store-chip">
                    ${s.name}
                    <button aria-label="${s.name} 제거"
                      @click=${(): void => this.toggleEmartStore({ ...s, area: '' })}>
                      ${icon('x', 13)}
                    </button>
                  </span>
                `,
              )}
            </div>
          `
        : nothing}
      ${allStores.length > 0
        ? html`
            <input
              .value=${this.emartStoreQuery}
              placeholder="지점 검색"
              style="background:var(--bg);padding:10px 14px;margin-bottom:8px"
              @input=${(e: Event): void => { this.emartStoreQuery = (e.target as HTMLInputElement).value; }}
            >
            ${stores.length > 0
              ? html`
                  <div class="store-list">
                    ${stores.map(
                      (s) => html`
                        <button class="store-row" @click=${(): void => this.toggleEmartStore(s)}>
                          <span class="names">
                            <b>${s.name}</b>
                            <span class="sub">${s.area}</span>
                          </span>
                        </button>
                      `,
                    )}
                  </div>
                `
              : html`<p class="sub" style="margin:4px 0 0">검색 결과가 없어요</p>`}
          `
        : nothing}

      <div class="frow" style="min-height:auto;margin-top:14px">
        <span style="font-weight:700;font-size:0.92rem">재고 있을 때만 알림</span>
        <label class="switch">
          <input type="checkbox" ?checked=${this.config.onlyInStock !== false}
            @change=${(e: Event): void => {
              this.applyConfig({ ...this.config, onlyInStock: (e.target as HTMLInputElement).checked });
            }}>
          <span class="knob"></span>
        </label>
      </div>
      <p class="sub" style="margin:6px 0 0">끄면 재고가 없어도 매회 알림이 와요</p>
    `;
  }

  // 주식 - 선택된 종목 표시 또는 코드·이름 검색
  private renderStockConfig(): TemplateResult {
    if (typeof this.config.code === 'string' && this.config.code !== '') {
      return html`
        <div class="stock-sel">
          <span class="names">
            <b>${this.config.name}</b>
            <span class="sub">
              ${this.config.code} · ${STOCK_KIND_LABELS[String(this.config.kind)] ?? ''}
            </span>
          </span>
          <button class="btn-ghost" @click=${(): void => {
            this.config = {};
            this.stockQuery = '';
            this.stockResults = [];
          }}>변경</button>
        </div>
      `;
    }
    return html`
      <input
        .value=${this.stockQuery}
        placeholder="종목명 또는 코드로 검색"
        style="background:var(--bg)"
        @input=${(e: Event): void => this.onStockQuery(e)}
      >
      ${this.stockResults.map(
        (s) => html`
          <button class="result-row" @click=${(): void => {
            this.applyConfig({ code: s.code, name: s.name, kind: s.kind });
          }}>
            <b>${s.name}</b>
            <span class="code">${s.code}</span>
            <span class="kind-badge">${STOCK_KIND_LABELS[s.kind] ?? s.kind}</span>
          </button>
        `,
      )}
      ${this.stockQuery.trim() !== '' && this.stockResults.length === 0
        ? html`<p class="sub" style="margin:10px 0 0">검색 결과가 없어요</p>`
        : nothing}
    `;
  }

  private renderForm(): TemplateResult {
    const info = findProvider(this.categoryId, this.providerId);
    if (!info) return html`${nothing}`;
    return html`
      <div class="block card target">
        ${providerAvatar(info.provider, 44)}
        <span class="names">
          <b>${info.provider.label}</b>
          <span class="sub">${info.category.label}</span>
        </span>
        ${this.editId === null
          ? html`
              <button class="btn-ghost change" @click=${(): void => { location.hash = '#/watch/new'; }}>변경</button>
            `
          : nothing}
      </div>

      <div class="block card">
        ${this.renderProviderConfig()}
      </div>

      <div class="block card">
        ${this.categoryId === 'movie'
          ? html`
              <div class="frow" style="min-height:auto;margin-bottom:8px">
                <span style="font-weight:700;font-size:0.92rem">알림·수집 주기 설정</span>
              </div>
            `
          : nothing}
        <schedule-editor
          .rules=${this.rules}
          @change=${(e: CustomEvent<ScheduleRule[]>): void => { this.rules = e.detail; }}
        ></schedule-editor>
      </div>

      ${MOVIE_PROVIDERS.includes(this.providerId) ? this.renderCgvEndsCard() : this.renderEndsCard()}

      <div class="block">
        <button class="btn-primary" ?disabled=${this.saving} @click=${(): void => void this.save()}>
          ${this.editId === null ? '알림 만들기' : '저장'}
        </button>
        ${this.editId !== null
          ? html`
              <button class="btn-ghost danger-btn" @click=${(): void => void this.removeWatch()}>
                알림 삭제
              </button>
            `
          : nothing}
      </div>

      <calendar-sheet></calendar-sheet>
      <time-wheel-sheet></time-wheel-sheet>
    `;
  }

  // CGV - 종료 시점은 마지막 선택 날짜의 종료 시각으로 자동 설정
  private renderCgvEndsCard(): TemplateResult {
    const dates = (this.config.dates ?? []) as string[];
    const last = [...dates].sort().at(-1);
    const end = (this.config.end as string | undefined) ?? '23:59';
    return html`
      <div class="block card">
        <div class="frow" style="min-height:auto">
          <span style="font-weight:700;font-size:0.92rem">종료 시점</span>
          <span class="sub">
            ${last ? `${this.dateLabel(last)} ${end}` : '날짜 선택 시 자동 설정'}
          </span>
        </div>
        <p class="sub" style="margin:8px 0 0">마지막 선택 날짜의 종료 시각에 자동으로 종료돼요</p>
      </div>
    `;
  }

  private renderEndsCard(): TemplateResult {
    return html`
      <div class="block card">
        ${this.providerId === 'emart'
          ? html`
              <div class="frow" style="min-height:auto;margin-bottom:14px">
                <span style="font-weight:700;font-size:0.92rem">재고 확인되면 종료</span>
                <label class="switch">
                  <input type="checkbox" ?checked=${this.config.endOnStock === true}
                    @change=${(e: Event): void => {
                      this.applyConfig({ ...this.config, endOnStock: (e.target as HTMLInputElement).checked });
                    }}>
                  <span class="knob"></span>
                </label>
              </div>
            `
          : nothing}
        <div class="frow" style="min-height:auto">
          <span style="font-weight:700;font-size:0.92rem">종료 시점</span>
          <label class="switch">
            <input type="checkbox" ?checked=${this.endsEnabled}
              @change=${(e: Event): void => this.onEndsToggle(e)}>
            <span class="knob"></span>
          </label>
        </div>
        ${this.endsEnabled
          ? html`
              <div class="ends-body" style="margin-top:12px">
                <div class="frow">
                  <span class="lbl">날짜</span>
                  <span class="ctl">
                    <button class="pick-btn" @click=${(): void => void this.pickEndsDate()}>
                      ${this.endsDate !== '' ? this.dateLabel(this.endsDate) : '날짜 선택'}
                    </button>
                  </span>
                </div>
                <div class="frow">
                  <span class="lbl">시각</span>
                  <span class="ctl">
                    <button class="pick-btn" @click=${(): void => void this.pickEndsHour()}>
                      ${Number(this.endsHour)}시까지
                    </button>
                  </span>
                </div>
              </div>
            `
          : html`<p class="sub" style="margin:8px 0 0">설정하지 않으면 상시로 유지돼요</p>`}
      </div>
    `;
  }

  render(): TemplateResult {
    return html`
      <div class="top">
        <button class="btn-icon" aria-label="뒤로" @click=${(): void => this.back()}>
          ${icon('chevron-left', 22)}
        </button>
        <h1>${this.editId !== null ? '알림 수정' : this.picking ? '새 알림' : '알림 설정'}</h1>
      </div>
      ${this.picking ? this.renderPick() : this.renderForm()}
    `;
  }
}
