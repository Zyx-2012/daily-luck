// 使用 Windows GUI 子系统，避免运行 .exe 时弹出黑色控制台窗口。
// 仅对 Windows 目标生效；在非 Windows 平台上会被忽略。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use daily_luck::{
    pcl2_first_hash, pcl2_identify, pcl2_luck, pcl2_luck_with_first_hash, pclce_identify,
    pclce_luck,
};
use eframe::egui;
use rand::Rng;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

// ---------------------------------------------------------------------------
// Constants & palette
// ---------------------------------------------------------------------------

/// 在接下来 1000 天内寻找最近一天人品为 100 的日子。
const PERFECT_WINDOW_DAYS: i64 = 1000;

/// 启动时自动计算的 365 天结果（从今天起连续 365 天）。
const YEAR_TABLE_DAYS: i64 = 365;

/// 识别码数量上限（防卡死）。
const MAX_SEARCH_COUNT: i64 = 2000;

/// 搜索天数上限（防卡死）。
const MAX_SEARCH_DAYS: i64 = 3650;

/// 开发者模式解除上限后的宽松上限。
const DEV_UNLOCKED_MAX: i64 = 100_000_000;

/// 结果列表最多展示的行数。
const MAX_RESULT_ROWS: usize = 50;

/// 断点续搜时缓存写入的粒度（每计算多少个识别码写盘一次）。
const CACHE_CHUNK: usize = 100;

/// 品牌色（唯一主题色）——深蓝。
const BRAND: egui::Color32 = egui::Color32::from_rgb(0x2B, 0x5F, 0xD9);
/// 次要信息灰。
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x9A, 0x9A, 0x9A);
/// 评分状态高亮色——绿色（满分/高分）。
const HIGHLIGHT: egui::Color32 = egui::Color32::from_rgb(0x2E, 0xCC, 0x71);

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// 365 天结果表中的一行。
struct DayRow {
    date: NaiveDate,
    pcl2: Option<u32>,
    pclce: Option<u32>,
}

/// 最近一个满分日的计算结果。
#[derive(Clone)]
struct PerfectInfo {
    /// 距今天的天数（今天为 0）。
    days_from_today: i64,
    /// 满分日日期。
    date: NaiveDate,
}

/// 结果排序方式（单选）。前两种是“运气最好”，后四种是“运气最差”的度量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortMode {
    /// 满分天数最多。
    PerfectDesc,
    /// 平均人品最高。
    AvgDesc,
    /// 满分天数最少。
    PerfectAsc,
    /// 0 分天数最多。
    ZeroDesc,
    /// 平均人品最低。
    AvgAsc,
    /// 距离起始日期最近的满分日最远（无满分日的排最前）。
    PerfectFar,
}

impl SortMode {
    const ALL: [SortMode; 6] = [
        Self::PerfectDesc,
        Self::AvgDesc,
        Self::PerfectAsc,
        Self::ZeroDesc,
        Self::AvgAsc,
        Self::PerfectFar,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::PerfectDesc => "运气最好 · 满分天数最多",
            Self::AvgDesc => "运气最好 · 平均人品最高",
            Self::PerfectAsc => "运气最差 · 满分天数最少",
            Self::ZeroDesc => "运气最差 · 0 分天数最多",
            Self::AvgAsc => "运气最差 · 平均人品最低",
            Self::PerfectFar => "运气最差 · 最近满分日最远",
        }
    }
}

/// 单个识别码在日期范围内的运气统计。
#[derive(Clone)]
struct SearchRow {
    id: String,
    /// 窗口内平均人品。
    avg_score: f64,
    /// 最高人品首次出现的日期。
    best_date: NaiveDate,
    /// 最高人品首次出现日距起始日期的天数。
    best_offset: i64,
    /// 窗口内人品为 100 的天数。
    perfect_count: usize,
    /// 窗口内人品为 0 的天数。
    zero_count: usize,
    /// 最近（最早出现）满分日距起始日期的天数；窗口内无满分则为 None。
    first_perfect_offset: Option<i64>,
}

/// 后台搜索线程的输入。
struct SearchRequest {
    start: NaiveDate,
    days: i64,
    count: usize,
}

/// 后台搜索线程发回主线程的消息。
enum SearchMsg {
    /// (已完成识别码数, 总数)。
    Progress(usize, usize),
    Done(Vec<SearchRow>),
}

/// 可调搜索参数。
#[derive(Clone)]
struct SearchParams {
    /// 起始日期（YYYY-MM-DD，默认今天）。
    start_date_text: String,
    /// 从起始日期起计算多少天。
    days: i64,
    /// 搜索的识别码数量。
    count: i64,
}

/// 开发者设置。
#[derive(Clone, Copy)]
struct DevSettings {
    /// 是否已通过确认弹窗解除上限。
    unlocked: bool,
    /// 断点续搜：是否缓存搜索结果。
    use_cache: bool,
}

impl Default for DevSettings {
    fn default() -> Self {
        Self {
            unlocked: false,
            use_cache: false,
        }
    }
}

struct DailyLuckApp {
    pcl2_id: String,
    pclce_id: String,
    today: NaiveDate,
    pcl2_today: Option<u32>,
    pclce_today: Option<u32>,
    pcl2_perfect: Option<PerfectInfo>,
    pclce_perfect: Option<PerfectInfo>,
    year_table: Vec<DayRow>,
    show_year_table: bool,
    params: SearchParams,
    /// 结果排序方式（单选）。
    sort_mode: SortMode,
    /// 结果列表当前页码（0 起）。
    result_page: usize,
    /// 后台搜索是否进行中。
    search_running: bool,
    search_results: Vec<SearchRow>,
    /// (已完成, 总数)。
    search_progress: (usize, usize),
    search_rx: Option<Receiver<SearchMsg>>,
    /// 开发者设置。
    dev: DevSettings,
    /// 是否显示开发者模式确认弹窗。
    show_dev_confirm: bool,
    /// 是否显示开发者设置面板。
    show_dev_panel: bool,
    /// 临时状态提示（缓存加载/删除等）。
    status_msg: Option<String>,
    /// 用户级缓存目录。
    cache_dir: PathBuf,
    auto_load_done: bool,
}

impl Default for DailyLuckApp {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            pcl2_id: String::new(),
            pclce_id: String::new(),
            today,
            pcl2_today: None,
            pclce_today: None,
            pcl2_perfect: None,
            pclce_perfect: None,
            year_table: Vec::new(),
            show_year_table: false,
            params: SearchParams {
                start_date_text: today.format("%Y-%m-%d").to_string(),
                days: YEAR_TABLE_DAYS,
                count: 100,
            },
            sort_mode: SortMode::AvgDesc,
            result_page: 0,
            search_running: false,
            search_results: Vec::new(),
            search_progress: (0, 0),
            search_rx: None,
            dev: DevSettings::default(),
            show_dev_confirm: false,
            show_dev_panel: false,
            status_msg: None,
            cache_dir: user_cache_dir(),
            auto_load_done: false,
        }
    }
}

impl DailyLuckApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 修复中文显示 bug：egui 默认字体不含 CJK 字形，需加载系统中文字体
        // 作为回退，否则界面上的中文会显示为方块。
        setup_cjk_font(&cc.egui_ctx);

        let mut app = Self::default();
        // 启动时自动读取本机识别码并计算全部结果。
        app.try_auto_load();
        app.compute_all();
        app
    }

    fn try_auto_load(&mut self) {
        if self.auto_load_done {
            return;
        }

        // Try to read PCL2 identifier from registry
        #[cfg(windows)]
        {
            if self.pcl2_id.is_empty() {
                if let Ok(id) = pcl2_identify() {
                    self.pcl2_id = id;
                }
            }

            // Try to read PCLCE identifier from WMI
            if self.pclce_id.is_empty() {
                if let Ok(id) = pclce_identify() {
                    self.pclce_id = id;
                }
            }
        }

        self.auto_load_done = true;
    }

    /// 重新计算全部结果：今日人品、1000 天内最近满分日、365 天结果表。
    fn compute_all(&mut self) {
        self.today = Local::now().date_naive();

        let pcl2_id = self.pcl2_id.trim();
        let pclce_id = self.pclce_id.trim();

        if pcl2_id.is_empty() {
            self.pcl2_today = None;
            self.pcl2_perfect = None;
        } else {
            self.pcl2_today = Some(luck_for_date(pcl2_luck, pcl2_id, self.today));
            self.pcl2_perfect =
                find_first_perfect(pcl2_luck, pcl2_id, self.today, PERFECT_WINDOW_DAYS);
        }

        if pclce_id.is_empty() {
            self.pclce_today = None;
            self.pclce_perfect = None;
        } else {
            self.pclce_today = Some(luck_for_date(pclce_luck, pclce_id, self.today));
            self.pclce_perfect =
                find_first_perfect(pclce_luck, pclce_id, self.today, PERFECT_WINDOW_DAYS);
        }

        self.year_table = (0..YEAR_TABLE_DAYS)
            .map(|offset| {
                let date = self.today + Duration::days(offset);
                DayRow {
                    date,
                    pcl2: if pcl2_id.is_empty() {
                        None
                    } else {
                        Some(luck_for_date(pcl2_luck, pcl2_id, date))
                    },
                    pclce: if pclce_id.is_empty() {
                        None
                    } else {
                        Some(luck_for_date(pclce_luck, pclce_id, date))
                    },
                }
            })
            .collect();
    }

    /// 当前上限：(天数上限, 数量上限)。开发者确认解锁后放宽。
    fn limits(&self) -> (i64, i64) {
        if self.dev.unlocked {
            (DEV_UNLOCKED_MAX, DEV_UNLOCKED_MAX)
        } else {
            (MAX_SEARCH_DAYS, MAX_SEARCH_COUNT)
        }
    }

    /// 启动一次后台搜索（使用随机识别码计算）。
    fn start_search(&mut self) {
        if self.search_running {
            return;
        }
        let start = parse_date_input(&self.params.start_date_text).unwrap_or(self.today);
        let (max_days, max_count) = self.limits();
        let days = self.params.days.clamp(1, max_days);
        let count = self.params.count.clamp(1, max_count) as usize;

        let (tx, rx) = std::sync::mpsc::channel();
        self.search_rx = Some(rx);
        self.search_running = true;
        self.search_progress = (0, count);
        self.search_results.clear();
        self.result_page = 0;

        // 断点续搜：若开启，把缓存路径传给线程以便增量写盘 / 续算。
        let use_cache = self.dev.use_cache;
        let cache_path = if use_cache {
            cache_file_path(&self.cache_dir, start, days, count as i64)
        } else {
            None
        };

        std::thread::spawn(move || {
            run_search(
                SearchRequest { start, days, count },
                tx,
                use_cache,
                cache_path,
                CACHE_CHUNK,
            );
        });
    }

    /// 每帧拉取后台线程的消息并应用。
    fn poll_search(&mut self) {
        let mut progress = None;
        let mut done = None;
        if let Some(rx) = &self.search_rx {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SearchMsg::Progress(d, t) => progress = Some((d, t)),
                    SearchMsg::Done(rows) => done = Some(rows),
                }
            }
        }
        if let Some(p) = progress {
            self.search_progress = p;
        }
        if let Some(rows) = done {
            self.search_results = rows;
            self.search_running = false;
            self.search_progress = (0, 0);
            self.search_rx = None;
            self.result_page = 0;
        }
    }

    // -----------------------------------------------------------------------
    // UI builders
    // -----------------------------------------------------------------------

    /// 顶部“满分日倒计时”信息卡片（内部强制竖排，避免在水平布局中
    /// 内容被横向挤成一排而溢出窗口边缘）。
    fn perfect_card(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        perfect: &Option<PerfectInfo>,
        today_score: Option<u32>,
        id_empty: bool,
        show_year_button: bool,
    ) {
        card_frame(ui).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_min_width(200.0);
                ui.label(egui::RichText::new(title).small().color(MUTED).strong());
                ui.add_space(4.0);

                let (big, sub) = match perfect {
                    Some(info) if info.days_from_today == 0 => (
                        "今天".to_string(),
                        format!("{} · 100 人品", info.date.format("%Y-%m-%d")),
                    ),
                    Some(info) => (
                        format!("{} 天后", info.days_from_today),
                        format!("{} · 100 人品", info.date.format("%Y-%m-%d")),
                    ),
                    None if id_empty => ("--".to_string(), "请输入识别码".to_string()),
                    None => (
                        "--".to_string(),
                        format!("{PERFECT_WINDOW_DAYS} 天内未出现 100"),
                    ),
                };
                ui.label(egui::RichText::new(big).size(36.0).color(BRAND).strong());
                ui.add_space(2.0);
                ui.label(egui::RichText::new(sub).small().color(MUTED));
                let today_text = today_score
                    .map(|s| format!("今日: {s}"))
                    .unwrap_or_else(|| "今日: --".to_string());
                ui.label(egui::RichText::new(today_text).small().color(MUTED));

                if show_year_button {
                    ui.add_space(8.0);
                    if ui.add(secondary_button("显示365天结果")).clicked() {
                        self.show_year_table = true;
                    }
                }
            });
        });
    }

    /// 搜索参数卡片 + 主按钮 + 排序选择 + 开发者设置入口。
    fn search_panel(&mut self, ui: &mut egui::Ui) {
        let (max_days, max_count) = self.limits();
        let limit_hint = if self.dev.unlocked {
            format!("（开发者模式，上限 {}）", DEV_UNLOCKED_MAX)
        } else {
            format!("（上限 {MAX_SEARCH_DAYS} / {MAX_SEARCH_COUNT}）")
        };

        card_frame(ui).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("搜索参数")
                        .small()
                        .color(MUTED)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("起始日期");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.params.start_date_text)
                            .desired_width(104.0),
                    );
                    ui.add_space(8.0);
                    ui.label("天数");
                    ui.add(
                        egui::DragValue::new(&mut self.params.days)
                            .range(1..=max_days)
                            .speed(1.0),
                    );
                    ui.add_space(8.0);
                    ui.label("识别码数量");
                    ui.add(
                        egui::DragValue::new(&mut self.params.count)
                            .range(1..=max_count)
                            .speed(1.0),
                    );
                });
                ui.add_space(8.0);

                // 排序方式单选
                ui.horizontal(|ui| {
                    ui.label("排序方式");
                    egui::ComboBox::from_id_source("sort_mode_combo")
                        .width(220.0)
                        .selected_text(self.sort_mode.label())
                        .show_ui(ui, |ui| {
                            for mode in SortMode::ALL {
                                ui.selectable_value(&mut self.sort_mode, mode, mode.label());
                            }
                        });
                });
                ui.add_space(10.0);

                if self.search_running {
                    let (done, total) = self.search_progress;
                    let frac = if total == 0 {
                        0.0
                    } else {
                        done as f32 / total as f32
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .text(format!("搜索中… {done} / {total}"))
                            .animate(true),
                    );
                } else {
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("使用随机识别码计算")).clicked() {
                            self.start_search();
                        }
                        ui.add_space(12.0);
                        if ui.add(secondary_button("开发者设置")).clicked() {
                            if self.dev.unlocked {
                                self.show_dev_panel = !self.show_dev_panel;
                            } else {
                                self.show_dev_confirm = true;
                            }
                        }
                    });
                }
                if !self.search_running {
                    ui.label(egui::RichText::new(limit_hint).small().color(MUTED));
                }
            });
        });
    }

    /// 搜索结果表格（按当前排序方式重排，支持翻页）。
    fn results_panel(&mut self, ui: &mut egui::Ui) {
        if self.search_results.is_empty() {
            return;
        }
        // 按当前排序方式重排（稳定排序，切换排序立即生效，无需重新搜索）。
        sort_rows(&mut self.search_results, self.sort_mode);

        let total = self.search_results.len();
        let total_pages = total.div_ceil(MAX_RESULT_ROWS).max(1);
        self.result_page = self.result_page.min(total_pages - 1);
        let page = self.result_page;

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(format!(
                "随机识别码计算结果 · 共 {total} 个 · 排序：{}",
                self.sort_mode.label()
            ))
            .strong(),
        );
        ui.add_space(6.0);

        let start = page * MAX_RESULT_ROWS;
        let page_rows: Vec<&SearchRow> =
            self.search_results.iter().skip(start).take(MAX_RESULT_ROWS).collect();

        egui::ScrollArea::vertical()
            .max_height(240.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("search_results_grid")
                    .striped(true)
                    .min_col_width(0.0)
                    .show(ui, |ui| {
                        ui.strong("识别码");
                        ui.strong("平均人品");
                        ui.strong("满分天数");
                        ui.strong("最佳日期");
                        ui.end_row();

                        for row in page_rows {
                            ui.label(egui::RichText::new(&row.id).monospace());
                            let avg_text = if (row.avg_score - row.avg_score.floor()).abs() < 1e-9 {
                                format!("{}", row.avg_score as i64)
                            } else {
                                format!("{:.1}", row.avg_score)
                            };
                            ui.label(avg_text);
                            if row.perfect_count > 0 {
                                ui.label(
                                    egui::RichText::new(row.perfect_count.to_string())
                                        .color(HIGHLIGHT)
                                        .strong(),
                                );
                            } else {
                                ui.label(egui::RichText::new("0").color(MUTED));
                            }
                            ui.label(format!(
                                "{} ({} 天后)",
                                row.best_date.format("%Y-%m-%d"),
                                row.best_offset
                            ));
                            ui.end_row();
                        }
                    });
            });

        // 翻页控件
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let prev = ui.add_enabled(page > 0, secondary_button("上一页")).clicked();
            ui.label(
                egui::RichText::new(format!("第 {} / {} 页", page + 1, total_pages))
                    .color(MUTED),
            );
            let next = ui
                .add_enabled(page + 1 < total_pages, secondary_button("下一页"))
                .clicked();
            if prev && page > 0 {
                self.result_page = page - 1;
            }
            if next && page + 1 < total_pages {
                self.result_page = page + 1;
            }
        });
    }

    /// 365 天结果弹窗的内容。返回是否点击了“关闭”按钮
    /// （窗口右上角的 X 由 `Window::open` 自动处理）。
    fn year_table_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut close_clicked = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "未来 {} 天 · 起始 {}",
                    YEAR_TABLE_DAYS,
                    self.today.format("%Y-%m-%d")
                ))
                .strong(),
            );
            if ui.button("关闭").clicked() {
                close_clicked = true;
            }
        });
        ui.separator();

        if self.year_table.is_empty() {
            ui.label("尚未计算，请先点击“计算”。");
            return close_clicked;
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("year_table_grid")
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("日期");
                        ui.strong("星期");
                        ui.strong("PCL2");
                        ui.strong("PCLCE");
                        ui.end_row();

                        for row in &self.year_table {
                            let is_today = row.date == self.today;
                            let is_perfect =
                                row.pcl2 == Some(100) || row.pclce == Some(100);

                            let date_text = if is_today {
                                format!("{} (今天)", row.date.format("%Y-%m-%d"))
                            } else {
                                row.date.format("%Y-%m-%d").to_string()
                            };
                            // 使用主题默认文本色而非硬编码白色，避免浅色主题下
                            // 未满分行的日期“看不见”（数据仍在，可正常复制）。
                            let date_rich = if is_perfect {
                                egui::RichText::new(date_text)
                                    .strong()
                                    .color(HIGHLIGHT)
                            } else {
                                egui::RichText::new(date_text).strong()
                            };
                            ui.label(date_rich);

                            ui.label(weekday_cn(row.date));

                            score_cell(ui, row.pcl2);
                            score_cell(ui, row.pclce);
                            ui.end_row();
                        }
                    });
            });
        close_clicked
    }

    /// 开发者设置面板（断点续搜 + 加载/删除缓存 + 上限状态）。
    fn dev_panel(&mut self, ui: &mut egui::Ui) {
        card_frame(ui).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("开发者设置").small().color(MUTED).strong());
                ui.add_space(6.0);

                ui.checkbox(&mut self.dev.use_cache, "断点续搜（缓存搜索结果）");
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("缓存目录：{}", self.cache_dir.display()))
                        .small()
                        .color(MUTED),
                );

                // 当前参数对应的缓存文件
                let start = parse_date_input(&self.params.start_date_text).unwrap_or(self.today);
                let (max_days, max_count) = self.limits();
                let days = self.params.days.clamp(1, max_days);
                let count = self.params.count.clamp(1, max_count);
                let cache_path = cache_file_path(&self.cache_dir, start, days, count);

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("加载缓存").clicked() {
                        self.status_msg = match cache_path.as_ref().and_then(load_cache_file) {
                            Some(rows) if !rows.is_empty() => {
                                self.search_results = rows;
                                self.result_page = 0;
                                Some(format!("已加载缓存：{} 条", self.search_results.len()))
                            }
                            _ => Some("加载缓存失败：文件不存在或已损坏".to_string()),
                        };
                    }
                    if ui.button("删除缓存").clicked() {
                        self.status_msg = match cache_path.as_deref().map(delete_cache_file) {
                            Some(Ok(())) => Some("缓存已删除".to_string()),
                            Some(Err(e)) => Some(format!("删除缓存失败：{e}")),
                            None => Some("无法确定缓存路径".to_string()),
                        };
                    }
                });

                if self.dev.unlocked {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("已解除上限（开发者模式）")
                            .color(HIGHLIGHT)
                            .small(),
                    );
                }
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// 卡片式容器。
fn card_frame(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::none()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(egui::Stroke::new(
            1.0_f32,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(16.0, 12.0))
}

/// 主操作按钮：品牌色实心、白字、圆角。
fn primary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE).strong())
        .fill(BRAND)
        .rounding(8.0)
        .min_size(egui::vec2(0.0, 34.0))
}

/// 次要按钮：浅灰描边、圆角。
fn secondary_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0_f32, MUTED))
        .rounding(8.0)
        .min_size(egui::vec2(0.0, 32.0))
}

/// 渲染一个带颜色的人品分数（None 显示 "--"）。
fn score_cell(ui: &mut egui::Ui, score: Option<u32>) {
    match score {
        Some(100) => {
            ui.label(egui::RichText::new("100").color(HIGHLIGHT).strong());
        }
        Some(s) if s >= 90 => {
            ui.label(egui::RichText::new(s.to_string()).color(HIGHLIGHT));
        }
        Some(s) => {
            ui.label(s.to_string());
        }
        None => {
            ui.label(egui::RichText::new("--").color(MUTED));
        }
    }
}

/// 在某一天计算某个算法的得分（y/m/d 从 `date` 提取）。
fn luck_for_date(
    scorer: fn(&str, i32, u32, u32) -> u32,
    id: &str,
    date: NaiveDate,
) -> u32 {
    scorer(id, date.year(), date.month(), date.day())
}

/// 在 `[start, start + lookahead)` 内寻找第一个人品为 100 的日期。
fn find_first_perfect(
    scorer: fn(&str, i32, u32, u32) -> u32,
    id: &str,
    start: NaiveDate,
    lookahead: i64,
) -> Option<PerfectInfo> {
    for offset in 0..lookahead {
        let date = start + Duration::days(offset);
        if luck_for_date(scorer, id, date) == 100 {
            return Some(PerfectInfo {
                days_from_today: offset,
                date,
            });
        }
    }
    None
}

fn weekday_cn(date: NaiveDate) -> &'static str {
    match date.weekday() {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

/// 解析 YYYY-MM-DD 文本；失败返回 None。
fn parse_date_input(s: &str) -> Option<NaiveDate> {
    let mut parts = s.trim().split('-');
    let y = parts.next()?.parse::<i32>().ok()?;
    let m = parts.next()?.parse::<u32>().ok()?;
    let d = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    NaiveDate::from_ymd_opt(y, m, d)
}

/// 生成一个随机 PCL2 格式识别码 XXXX-XXXX-XXXX-XXXX。
fn gen_random_id(rng: &mut impl Rng) -> String {
    format!(
        "{:04X}-{:04X}-{:04X}-{:04X}",
        rng.gen_range(0..=0xFFFF),
        rng.gen_range(0..=0xFFFF),
        rng.gen_range(0..=0xFFFF),
        rng.gen_range(0..=0xFFFF),
    )
}

/// 后台搜索线程主体：生成 `count` 个随机识别码，统计每个识别码在
/// `[start, start + days)` 内的运气指标，发回主线程（排序由主线程负责）。
/// 开启 `use_cache` 时，会先尝试加载同参数下的已有缓存并从断点续算，
/// 每完成 `chunk` 个识别码就增量落盘一次，最终完成后再覆盖写一份。
fn run_search(
    req: SearchRequest,
    tx: Sender<SearchMsg>,
    use_cache: bool,
    cache_path: Option<PathBuf>,
    chunk: usize,
) {
    // PCL2 的第一种子只与日期有关，与识别码无关 —— 预计算一次共享。
    let first_hashes: Vec<f64> = (0..req.days)
        .map(|offset| {
            let date = req.start + Duration::days(offset);
            pcl2_first_hash(date.year(), date.month(), date.day())
        })
        .collect();

    let mut rng = rand::thread_rng();
    let mut rows = Vec::with_capacity(req.count);

    // 尝试加载缓存并续算
    let mut loaded = 0usize;
    if use_cache {
        if let Some(path) = &cache_path {
            if let Some(existing) = load_cache_file(path) {
                loaded = existing.len().min(req.count);
                rows.extend(existing.into_iter().take(loaded));
                let _ = tx.send(SearchMsg::Progress(loaded, req.count));
            }
        }
    }

    for i in loaded..req.count {
        if (i + 1) % 10 == 0 && tx.send(SearchMsg::Progress(i + 1, req.count)).is_err() {
            return; // 主界面已关闭
        }
        let id = gen_random_id(&mut rng);
        let mut sum: u64 = 0;
        let mut best_score = 0u32;
        let mut best_date = req.start;
        let mut best_offset = 0i64;
        let mut perfect_count = 0usize;
        let mut zero_count = 0usize;
        let mut first_perfect_offset: Option<i64> = None;

        for (offset, &first_hash) in first_hashes.iter().enumerate() {
            let date = req.start + Duration::days(offset as i64);
            let score = pcl2_luck_with_first_hash(
                &id,
                date.year(),
                date.month(),
                date.day(),
                first_hash,
            );
            sum += u64::from(score);
            if score == 100 {
                perfect_count += 1;
                if first_perfect_offset.is_none() {
                    first_perfect_offset = Some(offset as i64);
                }
            }
            if score == 0 {
                zero_count += 1;
            }
            if score > best_score {
                best_score = score;
                best_date = date;
                best_offset = offset as i64;
            }
        }

        rows.push(SearchRow {
            id,
            avg_score: sum as f64 / req.days as f64,
            best_date,
            best_offset,
            perfect_count,
            zero_count,
            first_perfect_offset,
        });

        // 增量写缓存
        if use_cache && (i + 1) % chunk == 0 {
            if let Some(path) = &cache_path {
                let _ = save_cache_file(path, &rows);
            }
        }
    }

    // 完成最终写盘
    if use_cache {
        if let Some(path) = &cache_path {
            let _ = save_cache_file(path, &rows);
        }
    }

    let _ = tx.send(SearchMsg::Done(rows));
}

// ---------------------------------------------------------------------------
// Cache helpers
//
// 缓存存到用户级目录，避免在程序运行目录变化时找不到文件。
// Windows:  %LOCALAPPDATA%\daily-luck
// macOS:    ~/Library/Caches/daily-luck
// Linux:    ~/.cache/daily-luck
// ---------------------------------------------------------------------------

fn user_cache_dir() -> PathBuf {
    // Windows
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("daily-luck");
    }
    // macOS / Linux
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home.join("Library").join("Caches").join("daily-luck");
        }
        return home.join(".cache").join("daily-luck");
    }
    // 最后的 fallback（理论上不会走到这里）
    PathBuf::from(".daily-luck-cache")
}

fn cache_file_path(cache_dir: &PathBuf, start: NaiveDate, days: i64, count: i64) -> Option<PathBuf> {
    let dir = cache_dir;
    if let Err(e) = std::fs::create_dir_all(dir) {
        // GUI 应用中不打印到控制台，避免弹窗
        eprintln!("[daily-luck] 无法创建缓存目录 {dir:?}: {e}");
        return None;
    }
    let name = format!(
        "search_{}_{days}_{count}.cache",
        start.format("%Y%m%d")
    );
    Some(dir.join(name))
}

/// 行格式：id|avg_score|best_date|best_offset|perfect_count|zero_count|first_perfect_offset
fn save_cache_file(path: &PathBuf, rows: &[SearchRow]) -> Result<(), String> {
    let dir = path.parent().ok_or("cache path has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("create cache dir: {e}"))?;
    let tmp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| format!("create tmp: {e}"))?;
        for row in rows {
            let fpo = match row.first_perfect_offset {
                Some(v) => v.to_string(),
                None => "N".to_string(),
            };
            let line = format!(
                "{}|{}|{}|{}|{}|{}|{}\n",
                row.id,
                row.avg_score,
                row.best_date.format("%Y-%m-%d"),
                row.best_offset,
                row.perfect_count,
                row.zero_count,
                fpo
            );
            f.write_all(line.as_bytes()).map_err(|e| format!("write: {e}"))?;
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}

fn load_cache_file(path: &PathBuf) -> Option<Vec<SearchRow>> {
    let data = std::fs::read_to_string(path)
        .inspect_err(|e| {
            // GUI 应用中不打印到控制台，避免弹窗
            eprintln!("[daily-luck] 读缓存失败 {path:?}: {e}");
        })
        .ok()?;
    let mut rows = Vec::new();
    for (line_no, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 7 {
            // GUI 应用中不打印到控制台，避免弹窗
            eprintln!("[daily-luck] 缓存第 {line_no} 行格式错误，跳过");
            continue;
        }
        let id = parts[0].to_string();
        let avg_score: f64 = parts[1].parse().ok()?;
        let best_date = NaiveDate::parse_from_str(parts[2], "%Y-%m-%d").ok()?;
        let best_offset: i64 = parts[3].parse().ok()?;
        let perfect_count: usize = parts[4].parse().ok()?;
        let zero_count: usize = parts[5].parse().ok()?;
        let first_perfect_offset = if parts[6] == "N" {
            None
        } else {
            Some(parts[6].parse().ok()?)
        };
        rows.push(SearchRow {
            id,
            avg_score,
            best_date,
            best_offset,
            perfect_count,
            zero_count,
            first_perfect_offset,
        });
    }
    Some(rows)
}

fn delete_cache_file(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| format!("remove cache: {e}"))?;
    }
    Ok(())
}

/// 按排序方式对结果重排（稳定排序，原地修改）。
fn sort_rows(rows: &mut [SearchRow], mode: SortMode) {
    rows.sort_by(|a, b| {
        let by_id = a.id.cmp(&b.id);
        match mode {
            SortMode::PerfectDesc => b
                .perfect_count
                .cmp(&a.perfect_count)
                .then(b.avg_score.total_cmp(&a.avg_score))
                .then(by_id),
            SortMode::AvgDesc => b
                .avg_score
                .total_cmp(&a.avg_score)
                .then(b.perfect_count.cmp(&a.perfect_count))
                .then(by_id),
            SortMode::PerfectAsc => a
                .perfect_count
                .cmp(&b.perfect_count)
                .then(a.avg_score.total_cmp(&b.avg_score))
                .then(by_id),
            SortMode::ZeroDesc => b
                .zero_count
                .cmp(&a.zero_count)
                .then(a.avg_score.total_cmp(&b.avg_score))
                .then(by_id),
            SortMode::AvgAsc => a
                .avg_score
                .total_cmp(&b.avg_score)
                .then(a.perfect_count.cmp(&b.perfect_count))
                .then(by_id),
            SortMode::PerfectFar => {
                // 无满分日的排最前（最差）；有满分的按最近满分日距离降序。
                let key = |r: &SearchRow| match r.first_perfect_offset {
                    Some(off) => (1u8, -off),
                    None => (0u8, 0i64),
                };
                key(a)
                    .cmp(&key(b))
                    .then(a.avg_score.total_cmp(&b.avg_score))
                    .then(by_id)
            }
        }
    });
}

// ---------------------------------------------------------------------------
// CJK font support
//
// egui's bundled fonts (Ubuntu-Light / NotoEmoji) contain no CJK glyphs, so
// every Chinese label renders as tofu boxes unless a system font is loaded.
// We load the first available system CJK font and append it to both font
// families as a *fallback* (kept at the end of the list, so Latin text still
// uses the default fonts and only CJK characters fall through to it).
// ---------------------------------------------------------------------------

fn setup_cjk_font(ctx: &egui::Context) {
    let Some((bytes, index)) = load_system_cjk_font() else {
        // GUI 应用中不打印到控制台，避免弹窗
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("system-cjk".to_owned(), {
        let mut data = egui::FontData::from_owned(bytes);
        data.index = index;
        data
    });
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families
            .entry(family)
            .or_default()
            .push("system-cjk".to_owned());
    }
    ctx.set_fonts(fonts);
}

/// Look for a CJK-capable font in the usual system locations.
/// Returns the font bytes and the face index to use.
fn load_system_cjk_font() -> Option<(Vec<u8>, u32)> {
    const CANDIDATES: &[(&str, u32)] = &[
        // Windows
        (r"C:\Windows\Fonts\msyh.ttc", 0),   // 微软雅黑 (Microsoft YaHei)
        (r"C:\Windows\Fonts\Deng.ttf", 0),   // 等线 (DengXian)
        (r"C:\Windows\Fonts\simhei.ttf", 0), // 黑体 (SimHei)
        (r"C:\Windows\Fonts\simsun.ttc", 0), // 宋体 (SimSun)
        // macOS
        ("/System/Library/Fonts/PingFang.ttc", 0),
        ("/System/Library/Fonts/STHeiti Light.ttc", 0),
        // Linux
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
        ("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf", 0),
    ];
    for (path, index) in CANDIDATES {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((bytes, *index));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

impl eframe::App for DailyLuckApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 搜索期间持续重绘，以便接收后台线程的进度/结果消息。
        if self.search_running {
            ctx.request_repaint();
        }
        self.poll_search();

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);

                    // Title
                    ui.vertical_centered(|ui| {
                        ui.heading(
                            egui::RichText::new("今日人品 · PCL2 / PCLCE").size(26.0),
                        );
                    });
                    ui.add_space(16.0);

                    // 顶部：两并排信息卡片
                    ui.horizontal(|ui| {
                        // 克隆小数据避免借用冲突（perfect_card 需要 &mut self）
                        let pcl2_perfect = self.pcl2_perfect.clone();
                        let pclce_perfect = self.pclce_perfect.clone();
                        let pcl2_today = self.pcl2_today;
                        let pclce_today = self.pclce_today;
                        let pcl2_empty = self.pcl2_id.trim().is_empty();
                        let pclce_empty = self.pclce_id.trim().is_empty();

                        self.perfect_card(
                            ui,
                            "PCL2",
                            &pcl2_perfect,
                            pcl2_today,
                            pcl2_empty,
                            true, // “显示365天结果”放在这里（顶部卡片旁）
                        );
                        ui.add_space(12.0);
                        self.perfect_card(
                            ui,
                            "PCLCE",
                            &pclce_perfect,
                            pclce_today,
                            pclce_empty,
                            false,
                        );
                    });

                    ui.add_space(16.0);

                    // 输入区：placeholder 提示、紧凑同行
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.pcl2_id)
                                    .hint_text("输入 PCL2 识别码…")
                                    .desired_width(220.0),
                            )
                            .changed();
                        ui.add_space(8.0);
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut self.pclce_id)
                                    .hint_text("输入 PCLCE 识别码…")
                                    .desired_width(220.0),
                            )
                            .changed();
                    });
                    if changed {
                        self.compute_all();
                    }

                    ui.add_space(16.0);

                    // 搜索参数 + 按钮
                    self.search_panel(ui);

                    // 开发者设置面板（解锁后可展开）
                    if self.show_dev_panel {
                        ui.add_space(12.0);
                        self.dev_panel(ui);
                    }

                    // 状态提示
                    if let Some(msg) = &self.status_msg {
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new(msg).small().color(HIGHLIGHT));
                    }

                    ui.add_space(16.0);

                    // 搜索结果表格
                    self.results_panel(ui);

                    ui.add_space(12.0);

                    // Footer
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        let date_str = Local::now().format("%Y-%m-%d %A").to_string();
                        ui.label(egui::RichText::new(date_str).small().color(MUTED));
                    });
                });
        });

        // 365 天结果弹窗
        if self.show_year_table {
            let mut open = true;
            let mut close_clicked = false;
            egui::Window::new("365 天人品预测")
                .open(&mut open)
                .default_size([560.0, 420.0])
                .show(ctx, |ui| {
                    close_clicked = self.year_table_ui(ui);
                });
            if close_clicked {
                open = false;
            }
            self.show_year_table = open;
        }

        // 开发者模式确认弹窗
        if self.show_dev_confirm {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("开发者模式确认")
                .collapsible(false)
                .resizable(false)
                .default_size([360.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        egui::RichText::new(
                            "确认进入开发者模式并解除“天数 / 识别码数量”上限吗？\n\
                             解除后可使用更大的参数（可能显著增加计算量）。",
                        )
                        .size(14.0),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("确认解除")).clicked() {
                            confirm = true;
                        }
                        if ui.add(secondary_button("取消")).clicked() {
                            cancel = true;
                        }
                    });
                });
            if confirm {
                self.dev.unlocked = true;
                self.show_dev_panel = true;
                self.show_dev_confirm = false;
            }
            if cancel {
                self.show_dev_confirm = false;
            }
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([620.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "今日人品",
        options,
        Box::new(|cc| Ok(Box::new(DailyLuckApp::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weekday_cn_known_dates() {
        assert_eq!(weekday_cn(NaiveDate::from_ymd_opt(2025, 3, 16).unwrap()), "周日");
        assert_eq!(weekday_cn(NaiveDate::from_ymd_opt(2025, 3, 17).unwrap()), "周一");
        assert_eq!(weekday_cn(NaiveDate::from_ymd_opt(2025, 3, 22).unwrap()), "周六");
    }

    #[test]
    fn find_first_perfect_is_first_100_day() {
        let today = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        for id in ["ABCD-EFGH-1234-5678", "cafe-babe-dead-beef", "WEB-123456"] {
            let lookahead = 2000i64;
            let found = find_first_perfect(pcl2_luck, id, today, lookahead);
            assert!(
                found.is_some(),
                "expected at least one 100-score day for {id} in {lookahead} days"
            );
            let info = found.unwrap();
            assert_eq!(
                luck_for_date(pcl2_luck, id, info.date),
                100,
                "reported perfect day is not actually 100 for {id}"
            );
            for offset in 0..info.days_from_today {
                let date = today + Duration::days(offset);
                assert_ne!(
                    luck_for_date(pcl2_luck, id, date),
                    100,
                    "earlier day {date} was 100 for {id}"
                );
            }
        }
    }

    #[test]
    fn find_first_perfect_respects_lookahead() {
        let today = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        assert!(find_first_perfect(pcl2_luck, "ABCD-EFGH-1234-5678", today, 0).is_none());
    }

    #[test]
    fn luck_for_date_matches_lib() {
        let date = NaiveDate::from_ymd_opt(2025, 6, 15).unwrap();
        assert_eq!(
            luck_for_date(pcl2_luck, "ABCD-EFGH-1234-5678", date),
            pcl2_luck("ABCD-EFGH-1234-5678", 2025, 6, 15)
        );
        assert_eq!(
            luck_for_date(pclce_luck, "WXYZ-1234-ABCD-5678", date),
            pclce_luck("WXYZ-1234-ABCD-5678", 2025, 6, 15)
        );
    }

    #[test]
    fn parse_date_input_accepts_iso_and_rejects_garbage() {
        assert_eq!(
            parse_date_input("2026-08-18"),
            NaiveDate::from_ymd_opt(2026, 8, 18)
        );
        assert_eq!(parse_date_input(" 2026-1-5 "), NaiveDate::from_ymd_opt(2026, 1, 5));
        assert_eq!(parse_date_input(""), None);
        assert_eq!(parse_date_input("2026-13-01"), None); // 非法月份
        assert_eq!(parse_date_input("2026-08-18-extra"), None);
        assert_eq!(parse_date_input("today"), None);
    }

    #[test]
    fn gen_random_id_has_pcl2_shape() {
        let mut rng = rand::thread_rng();
        for _ in 0..20 {
            let id = gen_random_id(&mut rng);
            let groups: Vec<&str> = id.split('-').collect();
            assert_eq!(groups.len(), 4, "bad id {id}");
            assert!(
                groups
                    .iter()
                    .all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_hexdigit())),
                "bad id {id}"
            );
        }
    }

    #[test]
    fn run_search_computes_window_statistics() {
        let (tx, rx) = std::sync::mpsc::channel();
        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let days = 30;
        let req = SearchRequest {
            start,
            days,
            count: 40,
        };
        run_search(req, tx, false, None, CACHE_CHUNK);
        // run_search 会先发若干 Progress 消息，循环收到 Done 为止
        let mut got = None;
        for _ in 0..1000 {
            match rx.recv() {
                Ok(SearchMsg::Done(rows)) => {
                    got = Some(rows);
                    break;
                }
                Ok(SearchMsg::Progress(..)) => continue,
                Err(_) => break,
            }
        }
        let rows = got.expect("expected Done message");
        assert_eq!(rows.len(), 40);
        for row in &rows {
            // 统计量必须在窗口范围内且自洽
            assert!(row.best_offset >= 0 && row.best_offset < days);
            assert!(row.perfect_count <= days as usize);
            assert!(row.zero_count <= days as usize);
            assert_eq!(row.best_date, start + Duration::days(row.best_offset));
            assert!(row.first_perfect_offset.is_none() || row.first_perfect_offset.unwrap() < days);
            if let Some(off) = row.first_perfect_offset {
                // 最近满分日的得分必须确实是 100
                let date = start + Duration::days(off);
                assert_eq!(
                    pcl2_luck(&row.id, date.year(), date.month(), date.day()),
                    100,
                    "first_perfect_offset of {} is not actually 100",
                    row.id
                );
            }
        }
    }

    fn row_ids(rows: &[SearchRow]) -> Vec<&str> {
        rows.iter().map(|r| r.id.as_str()).collect()
    }

    fn sample_rows() -> Vec<SearchRow> {        let start = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        vec![
            SearchRow {
                id: "AAAA-0000-0000-0001".into(),
                avg_score: 50.0,
                best_date: start,
                best_offset: 0,
                perfect_count: 2,
                zero_count: 5,
                first_perfect_offset: Some(10),
            },
            SearchRow {
                id: "AAAA-0000-0000-0002".into(),
                avg_score: 90.0,
                best_date: start,
                best_offset: 3,
                perfect_count: 5,
                zero_count: 1,
                first_perfect_offset: Some(3),
            },
            SearchRow {
                id: "AAAA-0000-0000-0003".into(),
                avg_score: 10.0,
                best_date: start,
                best_offset: 1,
                perfect_count: 0,
                zero_count: 12,
                first_perfect_offset: None,
            },
            SearchRow {
                id: "AAAA-0000-0000-0004".into(),
                avg_score: 30.0,
                best_date: start,
                best_offset: 2,
                perfect_count: 3,
                zero_count: 8,
                first_perfect_offset: Some(20),
            },
        ]
    }

    #[test]
    fn sort_rows_supports_all_six_modes() {
        // 满分天数最多
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::PerfectDesc);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0002", "AAAA-0000-0000-0004", "AAAA-0000-0000-0001", "AAAA-0000-0000-0003"]);

        // 平均人品最高
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::AvgDesc);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0002", "AAAA-0000-0000-0001", "AAAA-0000-0000-0004", "AAAA-0000-0000-0003"]);

        // 满分天数最少
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::PerfectAsc);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0003", "AAAA-0000-0000-0001", "AAAA-0000-0000-0004", "AAAA-0000-0000-0002"]);

        // 0 分天数最多
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::ZeroDesc);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0003", "AAAA-0000-0000-0004", "AAAA-0000-0000-0001", "AAAA-0000-0000-0002"]);

        // 平均人品最低
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::AvgAsc);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0003", "AAAA-0000-0000-0004", "AAAA-0000-0000-0001", "AAAA-0000-0000-0002"]);

        // 最近满分日最远：无满分（0003）最前，然后 offset 20 > 10 > 3
        let mut rows = sample_rows();
        sort_rows(&mut rows, SortMode::PerfectFar);
        assert_eq!(row_ids(&rows), vec!["AAAA-0000-0000-0003", "AAAA-0000-0000-0004", "AAAA-0000-0000-0001", "AAAA-0000-0000-0002"]);
    }

    #[test]
    fn cache_round_trip_preserves_all_fields() {
        use std::time::SystemTime;
        let dir = std::env::temp_dir().join(format!(
            "daily_luck_test_cache_{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rows = sample_rows();
        let path = cache_file_path(&dir, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 30, 4);
        let path = path.expect("cache path");
        save_cache_file(&path, &rows).expect("save ok");

        let loaded = load_cache_file(&path).expect("load ok");
        assert_eq!(loaded.len(), rows.len());
        for (a, b) in loaded.iter().zip(rows.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.avg_score, b.avg_score);
            assert_eq!(a.best_date, b.best_date);
            assert_eq!(a.best_offset, b.best_offset);
            assert_eq!(a.perfect_count, b.perfect_count);
            assert_eq!(a.zero_count, b.zero_count);
            assert_eq!(a.first_perfect_offset, b.first_perfect_offset);
        }

        // 删除
        delete_cache_file(&path).expect("delete ok");
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_cache_rejects_corrupt_file() {
        use std::time::SystemTime;
        let dir = std::env::temp_dir().join(format!(
            "daily_luck_test_badcache_{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join("bad.cache");
        std::fs::create_dir_all(&dir).unwrap();
        // 写入非法行
        std::fs::write(&path, "not-a-valid-cache-line\n").unwrap();
        // 损坏文件应优雅返回 None，而不是 panic
        let loaded = load_cache_file(&path);
        assert!(loaded.is_none() || loaded.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
