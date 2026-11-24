//! 回档区块：只把世界里的一片区块换回某份快照的样子，别的地方不动。
//!
//! 地图上标出快照之后改过的区块，拖动框选，确认后换回去。动手前自动快照一份当前世界，
//! 回档错了从那份再回档回来。

use crate::prelude::*;
use nailong_core::snapshots::{Area, ChunkMap, ChunkRestore, ChunkState, Dimension};

pub(crate) struct ChunkRollback {
    game_dir: PathBuf,
    row_id: String,
    world: String,
    snapshot_id: String,
    snapshot_at: u64,
    dims: Vec<Dimension>,
    dim: usize,
    map: Option<ChunkMap>,
    map_rx: Option<mpsc::Receiver<Result<ChunkMap, String>>>,
    /// 地图中心（区块坐标）。
    center: egui::Vec2,
    /// 每个区块画多少像素。
    cell: f32,
    selection: Option<((i32, i32), (i32, i32))>,
    drag_from: Option<(i32, i32)>,
    jump_x: String,
    jump_z: String,
    rx: Option<mpsc::Receiver<Result<ChunkRestore, String>>>,
    error: Option<String>,
}

impl App {
    pub(crate) fn open_chunk_rollback(&mut self, world: &str, snapshot: &snapshots::Snapshot) {
        let Some(row) = self.worlds.row.as_ref() else {
            return;
        };
        let dims = snapshots::dimensions(snapshot);
        if dims.is_empty() {
            self.worlds.error = Some("这份快照里没有区块".into());
            return;
        }
        let mut dialog = ChunkRollback {
            game_dir: row.game_dir.clone(),
            row_id: row.id.clone(),
            world: world.to_string(),
            snapshot_id: snapshot.id.clone(),
            snapshot_at: snapshot.created_at,
            dims,
            dim: 0,
            map: None,
            map_rx: None,
            center: egui::Vec2::ZERO,
            cell: 8.0,
            selection: None,
            drag_from: None,
            jump_x: String::new(),
            jump_z: String::new(),
            rx: None,
            error: None,
        };
        load_map(&mut dialog, &self.ctx);
        self.chunks = Some(dialog);
    }

    /// 调试：等地图读完后框选改动最集中的一片（无头截图用）。
    #[cfg(debug_assertions)]
    pub(crate) fn debug_pick_chunks(&mut self) {
        if let Some(dialog) = &mut self.chunks
            && let Some(rx) = dialog.map_rx.take()
            && let Ok(Ok(map)) = rx.recv()
        {
            let (x, z) = map.focus;
            dialog.center = egui::vec2(x as f32 + 0.5, z as f32 + 0.5);
            dialog.selection = Some(((x - 6, z - 6), (x - 2, z - 2)));
            dialog.map = Some(map);
        }
    }

    pub(crate) fn poll_chunk_rollback(&mut self) {
        let Some(dialog) = &mut self.chunks else {
            return;
        };
        if let Some(rx) = &dialog.map_rx
            && let Ok(result) = rx.try_recv()
        {
            dialog.map_rx = None;
            match result {
                Ok(map) => {
                    dialog.center = egui::vec2(map.focus.0 as f32 + 0.5, map.focus.1 as f32 + 0.5);
                    dialog.map = Some(map);
                }
                Err(e) => dialog.error = Some(e),
            }
        }
        let Some(rx) = &dialog.rx else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        dialog.rx = None;
        match result {
            Ok(done) => {
                self.chunks = None;
                self.worlds.error = None;
                self.worlds.notice = Some(if done.chunks == 0 {
                    "选中的区块和快照一样，没有改动".into()
                } else {
                    format!("已回档 {} 个区块", done.chunks)
                });
                self.reload_saves();
            }
            Err(e) => dialog.error = Some(e),
        }
    }

    pub(crate) fn chunk_rollback_overlay(&mut self, ctx: &egui::Context) {
        let Some(row_id) = self.chunks.as_ref().map(|d| d.row_id.clone()) else {
            return;
        };
        let blocked = self.write_block(Some(&row_id));
        let Some(dialog) = &mut self.chunks else {
            return;
        };
        let working = dialog.rx.is_some();
        if working || dialog.map_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let mut close = false;
        let mut start = false;
        let mut reload = false;
        let ((), dismissed) = crate::widgets::dialog(ctx, "chunks", 620.0, |ui| {
            dialog_title(
                ui,
                "回档区块",
                &format!("{} · {}的快照", dialog.world, human_age(dialog.snapshot_at)),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                // 模组维度可能很多、名字很长：主世界、下界、末地用按钮，其余放进下拉。
                let mut wanted = dialog.dim;
                for index in 0..dialog.dims.len().min(3) {
                    if is_vanilla(&dialog.dims[index])
                        && chip(ui, &dialog.dims[index].label, index == dialog.dim).clicked()
                    {
                        wanted = index;
                    }
                }
                let others: Vec<usize> = (0..dialog.dims.len())
                    .filter(|&i| i >= 3 || !is_vanilla(&dialog.dims[i]))
                    .collect();
                if !others.is_empty() {
                    let current = if others.contains(&dialog.dim) {
                        short_text(&dialog.dims[dialog.dim].label, 18)
                    } else {
                        format!("其他 {} 个", others.len())
                    };
                    egui::ComboBox::from_id_salt("chunk_dims")
                        .selected_text(egui::RichText::new(current).size(13.0))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for &index in &others {
                                if ui
                                    .selectable_label(
                                        index == dialog.dim,
                                        &dialog.dims[index].label,
                                    )
                                    .clicked()
                                {
                                    wanted = index;
                                }
                            }
                        });
                }
                if wanted != dialog.dim && !working {
                    dialog.dim = wanted;
                    reload = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut jump = false;
                    for (label, text) in [("Z", &mut dialog.jump_z), ("X", &mut dialog.jump_x)] {
                        let field = ui.add(
                            egui::TextEdit::singleline(text)
                                .desired_width(64.0)
                                .hint_text(label)
                                .frame(crate::dialogs::add_account::field_frame()),
                        );
                        if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            jump = true;
                        }
                    }
                    ui.label(egui::RichText::new("跳到").size(13.0).color(t::sub()));
                    if jump
                        && let (Ok(x), Ok(z)) = (
                            dialog.jump_x.trim().parse::<i32>(),
                            dialog.jump_z.trim().parse::<i32>(),
                        )
                    {
                        let chunk = (x.div_euclid(16), z.div_euclid(16));
                        dialog.center = egui::vec2(chunk.0 as f32 + 0.5, chunk.1 as f32 + 0.5);
                        dialog.selection = Some((chunk, chunk));
                    }
                });
            });
            ui.add_space(10.0);
            let hovered = map_view(ui, dialog, !working);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (state, label) in [
                    (ChunkState::Changed, "改过"),
                    (ChunkState::Added, "新生成"),
                    (ChunkState::Removed, "已删除"),
                    (ChunkState::Same, "没变"),
                ] {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 2.0, state_color(state));
                    ui.label(egui::RichText::new(label).size(13.0).color(t::sub()));
                    ui.add_space(6.0);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = match hovered {
                        Some((x, z)) => format!("区块 {x}, {z} · 方块 {}, {}", x * 16, z * 16),
                        None => "拖动框选 · 右键拖动 · 滚轮缩放".into(),
                    };
                    ui.label(egui::RichText::new(text).size(13.0).color(t::dim()));
                });
            });
            ui.add_space(12.0);
            let picked = dialog
                .selection
                .map(|(a, b)| Area::new(&dialog.dims[dialog.dim].dir, a, b));
            let (changed, added) = match (&picked, &dialog.map) {
                (Some(area), Some(map)) => count_in(map, area),
                _ => (0, 0),
            };
            settings_card(ui, |ui| {
                let (title, detail) = match &picked {
                    None => ("还没选区块".to_string(), "在地图上拖出一块".to_string()),
                    Some(area) => (
                        format!(
                            "选中 {} × {}，{} 个区块",
                            area.max.0 - area.min.0 + 1,
                            area.max.1 - area.min.1 + 1,
                            area.count()
                        ),
                        if added > 0 {
                            format!("{changed} 个改过 · {added} 个新生成的会删掉重新生成")
                        } else {
                            format!("{changed} 个改过")
                        },
                    ),
                };
                ui.label(egui::RichText::new(title).size(14.0).family(bold_family()));
                ui.add_space(2.0);
                ui.label(egui::RichText::new(detail).size(13.0).color(t::sub()));
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("同步回档地形、实体与兴趣点，操作前自动快照")
                        .size(13.0)
                        .color(t::dim()),
                );
            });
            if let Some(reason) = blocked {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(reason).size(13.0).color(t::warn()));
            }
            if let Some(error) = &dialog.error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(error).size(13.0).color(t::danger()));
            }
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if working {
                    "正在回档…".to_string()
                } else {
                    format!("回档 {} 个区块", changed + added)
                };
                let ready =
                    picked.is_some() && blocked.is_none() && !working && dialog.map.is_some();
                if motion::primary_sized(ui, &label, ready, 140.0, 34.0, 13.0) {
                    start = true;
                }
                if motion::secondary_sized(ui, "取消", !working, 72.0, 34.0, 13.0).clicked() {
                    close = true;
                }
            });
        });
        if (dismissed || close) && !working {
            self.chunks = None;
            return;
        }
        if reload {
            dialog.selection = None;
            load_map(dialog, ctx);
        }
        if start && let Some((a, b)) = dialog.selection {
            let area = Area::new(&dialog.dims[dialog.dim].dir, a, b);
            let (dir, world, id) = (
                dialog.game_dir.clone(),
                dialog.world.clone(),
                dialog.snapshot_id.clone(),
            );
            let (tx, rx) = mpsc::channel();
            dialog.rx = Some(rx);
            dialog.error = None;
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let result =
                    snapshots::restore_chunks(&dir, &world, &id, &area).map_err(|e| e.to_string());
                let _ = tx.send(result);
                ctx.request_repaint();
            });
        }
    }
}

fn load_map(dialog: &mut ChunkRollback, ctx: &egui::Context) {
    let (tx, rx) = mpsc::channel();
    dialog.map = None;
    dialog.map_rx = Some(rx);
    dialog.error = None;
    let (dir, world, id, dim) = (
        dialog.game_dir.clone(),
        dialog.world.clone(),
        dialog.snapshot_id.clone(),
        dialog.dims[dialog.dim].dir.clone(),
    );
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = snapshots::chunk_map(&dir, &world, &id, &dim).map_err(|e| e.to_string());
        let _ = tx.send(result);
        ctx.request_repaint();
    });
}

/// 选区里有几个区块和快照不同：改过（含已删除）的、之后新生成的。
fn count_in(map: &ChunkMap, area: &Area) -> (usize, usize) {
    let mut changed = 0;
    let mut added = 0;
    for (&pos, &state) in &map.chunks {
        if !area.contains(pos) {
            continue;
        }
        match state {
            ChunkState::Changed | ChunkState::Removed => changed += 1,
            ChunkState::Added => added += 1,
            ChunkState::Same => {}
        }
    }
    (changed, added)
}

fn is_vanilla(dim: &Dimension) -> bool {
    matches!(dim.label.as_str(), "主世界" | "下界" | "末地")
}

fn state_color(state: ChunkState) -> egui::Color32 {
    match state {
        ChunkState::Changed => t::accent(),
        ChunkState::Added => t::warn(),
        ChunkState::Removed => t::danger(),
        ChunkState::Same => t::tint(t::dim(), 0.16),
    }
}

/// 画区块地图并处理框选、平移、缩放。返回鼠标下的区块。
fn map_view(
    ui: &mut egui::Ui,
    dialog: &mut ChunkRollback,
    interactive: bool,
) -> Option<(i32, i32)> {
    let size = egui::vec2(ui.available_width(), 340.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, motion::R_CARD, t::bg());
    painter.rect_stroke(
        rect,
        motion::R_CARD,
        egui::Stroke::new(1.0, t::hairline()),
        egui::StrokeKind::Inside,
    );
    let Some(map) = &dialog.map else {
        let text = if dialog.map_rx.is_some() {
            "正在读取区块…"
        } else {
            "没有区块"
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(13.0),
            t::sub(),
        );
        return None;
    };
    let cell = dialog.cell;
    let to_chunk = |pos: egui::Pos2, center: egui::Vec2| -> (i32, i32) {
        let v = (pos - rect.center()) / cell + center;
        (v.x.floor() as i32, v.y.floor() as i32)
    };

    if interactive {
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                // 以鼠标所在处为中心缩放。
                let anchor = response.hover_pos().unwrap_or(rect.center());
                let before = (anchor - rect.center()) / dialog.cell + dialog.center;
                dialog.cell = (dialog.cell * (1.0 + scroll * 0.004)).clamp(3.0, 32.0);
                let after = (anchor - rect.center()) / dialog.cell + dialog.center;
                dialog.center += before - after;
            }
        }
        if response.dragged_by(egui::PointerButton::Secondary)
            || response.dragged_by(egui::PointerButton::Middle)
        {
            dialog.center -= response.drag_delta() / dialog.cell;
        }
        if response.drag_started_by(egui::PointerButton::Primary) {
            dialog.drag_from = response
                .interact_pointer_pos()
                .map(|pos| to_chunk(pos, dialog.center));
        }
        if response.dragged_by(egui::PointerButton::Primary)
            && let (Some(from), Some(pos)) = (dialog.drag_from, response.interact_pointer_pos())
        {
            dialog.selection = Some((from, to_chunk(pos, dialog.center)));
        }
        if response.drag_stopped() {
            dialog.drag_from = None;
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let chunk = to_chunk(pos, dialog.center);
            dialog.selection = Some((chunk, chunk));
        }
    }

    let cell = dialog.cell;
    let hovered = response.hover_pos().map(|pos| {
        let v = (pos - rect.center()) / cell + dialog.center;
        (v.x.floor() as i32, v.y.floor() as i32)
    });
    let half = rect.size() / cell / 2.0;
    let (x0, x1) = (
        (dialog.center.x - half.x).floor() as i32,
        (dialog.center.x + half.x).ceil() as i32,
    );
    let (z0, z1) = (
        (dialog.center.y - half.y).floor() as i32,
        (dialog.center.y + half.y).ceil() as i32,
    );
    let to_screen =
        |x: i32, z: i32| rect.center() + (egui::vec2(x as f32, z as f32) - dialog.center) * cell;
    let gap = if cell >= 6.0 { 1.0 } else { 0.0 };
    for z in z0..=z1 {
        for x in x0..=x1 {
            if let Some(&state) = map.chunks.get(&(x, z)) {
                let min = to_screen(x, z);
                let r = egui::Rect::from_min_size(min, egui::vec2(cell - gap, cell - gap));
                painter.rect_filled(r, 0.0, state_color(state));
            }
        }
    }
    // 区域（32×32 区块）的边界，看出是哪个 r.X.Z.mca。
    if cell * 32.0 >= 48.0 {
        let line = egui::Stroke::new(1.0, t::tint(t::ink(), 0.12));
        for x in x0..=x1 {
            if x.rem_euclid(32) == 0 {
                let sx = to_screen(x, 0).x;
                painter.line_segment(
                    [egui::pos2(sx, rect.top()), egui::pos2(sx, rect.bottom())],
                    line,
                );
            }
        }
        for z in z0..=z1 {
            if z.rem_euclid(32) == 0 {
                let sy = to_screen(0, z).y;
                painter.line_segment(
                    [egui::pos2(rect.left(), sy), egui::pos2(rect.right(), sy)],
                    line,
                );
            }
        }
    }
    if let Some((a, b)) = dialog.selection {
        let min = to_screen(a.0.min(b.0), a.1.min(b.1));
        let max = to_screen(a.0.max(b.0) + 1, a.1.max(b.1) + 1);
        let r = egui::Rect::from_min_max(min, max);
        painter.rect_filled(r, 0.0, t::tint(t::ink(), 0.06));
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.5, t::ink()),
            egui::StrokeKind::Outside,
        );
    }
    if let Some((x, z)) = hovered {
        let r = egui::Rect::from_min_size(to_screen(x, z), egui::vec2(cell, cell));
        painter.rect_stroke(
            r,
            0.0,
            egui::Stroke::new(1.0, t::sub()),
            egui::StrokeKind::Outside,
        );
    }
    if map.changed == 0 {
        painter.text(
            rect.left_top() + egui::vec2(10.0, 8.0),
            egui::Align2::LEFT_TOP,
            "快照之后没有区块改过",
            egui::FontId::proportional(13.0),
            t::sub(),
        );
    }
    hovered
}
