use std::ops::Range;

use log::info;

use crate::caret::CaretType;
use crate::common::{interpolate_fix9_scale, Color, Direction, FadeDirection, FadeState, Rect};
use crate::components::boss_life_bar::BossLifeBar;
use crate::components::draw_common::Alignment;
use crate::components::flash::Flash;
use crate::components::hud::HUD;
use crate::components::inventory::InventoryUI;
use crate::components::stage_select::StageSelect;
use crate::components::water_renderer::WaterRenderer;
use crate::entity::GameEntity;
use crate::frame::{Frame, UpdateTarget};
use crate::framework::backend::SpriteBatchCommand;
use crate::framework::context::Context;
use crate::framework::error::GameResult;
use crate::framework::graphics::{draw_rect, BlendMode, FilterMode};
use crate::framework::ui::Components;
use crate::framework::{filesystem, graphics};
use crate::input::touch_controls::TouchControlType;
use crate::inventory::{Inventory, TakeExperienceResult};
use crate::map::WaterParams;
use crate::npc::boss::BossNPC;
use crate::npc::list::NPCList;
use crate::npc::{NPCLayer, NPC};
use crate::physics::{PhysicalEntity, OFFSETS};
use crate::player::{Player, TargetPlayer};
use crate::rng::XorShift;
use crate::scene::title_scene::TitleScene;
use crate::scene::Scene;
use crate::shared_game_state::{SharedGameState, TileSize};
use crate::stage::{BackgroundType, Stage};
use crate::text_script::{ConfirmSelection, ScriptMode, TextScriptExecutionState, TextScriptLine, TextScriptVM};
use crate::texture_set::SizedBatch;
use crate::weapon::bullet::BulletManager;
use crate::weapon::{Weapon, WeaponType};

pub struct GameScene {
    pub tick: u32,
    pub stage: Stage,
    pub water_params: WaterParams,
    pub water_renderer: WaterRenderer,
    pub boss_life_bar: BossLifeBar,
    pub stage_select: StageSelect,
    pub flash: Flash,
    pub inventory_ui: InventoryUI,
    pub hud_player1: HUD,
    pub hud_player2: HUD,
    pub frame: Frame,
    pub player1: Player,
    pub player2: Player,
    pub inventory_player1: Inventory,
    pub inventory_player2: Inventory,
    pub stage_id: usize,
    pub npc_list: NPCList,
    pub boss: BossNPC,
    pub bullet_manager: BulletManager,
    pub intro_mode: bool,
    tex_background_name: String,
    tex_tileset_name: String,
    tex_tileset_name_mg: String,
    tex_tileset_name_bg: String,
    map_name_counter: u16,
    skip_counter: u16,
    inventory_dim: f32,
}

#[derive(Debug, EnumIter, PartialEq, Eq, Hash, Copy, Clone)]
pub enum TileLayer {
    Background,
    Middleground,
    Foreground,
    Snack,
}

const FACE_TEX: &str = "Face";
const SWITCH_FACE_TEX: [&str; 4] = ["Face1", "Face2", "Face3", "Face4"];
const P2_LEFT_TEXT: &str = "< P2";
const P2_RIGHT_TEXT: &str = "P2 >";
const CUTSCENE_SKIP_WAIT: u16 = 50;

impl GameScene {
    pub fn new(state: &mut SharedGameState, ctx: &mut Context, id: usize) -> GameResult<Self> {
        info!("Loading stage {} ({})", id, &state.stages[id].map);
        let stage = Stage::load(&state.base_path, &state.stages[id], ctx)?;
        info!("Loaded stage: {}", stage.data.name);
        let mut water_params = WaterParams::new();
        let mut water_renderer = WaterRenderer::new();

        if let Ok(water_param_file) =
            filesystem::open(ctx, [&state.base_path, "Stage/", &state.stages[id].tileset.name, ".pxw"].join(""))
        {
            water_params.load_from(water_param_file)?;
            info!("Loaded water parameters file.");

            water_renderer.initialize(stage.map.find_water_regions());
        }

        let tex_background_name = stage.data.background.filename();
        let (tex_tileset_name, tex_tileset_name_mg, tex_tileset_name_bg) =
            if let Some(pxpack_data) = stage.data.pxpack_data.as_ref() {
                let t_fg = ["Stage/", &pxpack_data.tileset_fg].join("");
                let t_mg = ["Stage/", &pxpack_data.tileset_mg].join("");
                let t_bg = ["Stage/", &pxpack_data.tileset_bg].join("");

                (t_fg, t_mg, t_bg)
            } else {
                let tex_tileset_name = ["Stage/", &stage.data.tileset.filename()].join("");

                (tex_tileset_name.clone(), tex_tileset_name.clone(), tex_tileset_name)
            };

        Ok(Self {
            tick: 0,
            stage,
            water_params,
            water_renderer,
            player1: Player::new(state, ctx),
            player2: Player::new(state, ctx),
            inventory_player1: Inventory::new(),
            inventory_player2: Inventory::new(),
            boss_life_bar: BossLifeBar::new(),
            stage_select: StageSelect::new(),
            flash: Flash::new(),
            inventory_ui: InventoryUI::new(),
            hud_player1: HUD::new(Alignment::Left),
            hud_player2: HUD::new(Alignment::Right),
            frame: Frame {
                x: 0,
                y: 0,
                prev_x: 0,
                prev_y: 0,
                update_target: UpdateTarget::Player,
                target_x: 0,
                target_y: 0,
                wait: 16,
            },
            stage_id: id,
            npc_list: NPCList::new(),
            boss: BossNPC::new(),
            bullet_manager: BulletManager::new(),
            intro_mode: false,
            tex_background_name,
            tex_tileset_name,
            tex_tileset_name_mg,
            tex_tileset_name_bg,
            map_name_counter: 0,
            skip_counter: 0,
            inventory_dim: 0.0,
        })
    }

    pub fn display_map_name(&mut self, ticks: u16) {
        self.map_name_counter = ticks;
    }

    pub fn add_player2(&mut self) {
        self.player2.cond.set_alive(true);
        self.player2.cond.set_hidden(self.player1.cond.hidden());
        self.player2.x = self.player1.x;
        self.player2.y = self.player1.y;
        self.player2.vel_x = self.player1.vel_x;
        self.player2.vel_y = self.player1.vel_y;
    }

    pub fn drop_player2(&mut self) {
        self.player2.cond.set_alive(false);
    }

    fn draw_background(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, &self.tex_background_name)?;
        let scale = state.scale;
        let (frame_x, frame_y) = self.frame.xy_interpolated(state.frame_time);

        match self.stage.data.background_type {
            BackgroundType::TiledStatic => {
                graphics::clear(ctx, self.stage.data.background_color);

                let count_x = state.canvas_size.0 as usize / batch.width() + 1;
                let count_y = state.canvas_size.1 as usize / batch.height() + 1;

                for y in 0..count_y {
                    for x in 0..count_x {
                        batch.add((x * batch.width()) as f32, (y * batch.height()) as f32);
                    }
                }
            }
            BackgroundType::TiledParallax | BackgroundType::Tiled | BackgroundType::Waterway => {
                graphics::clear(ctx, self.stage.data.background_color);

                let (off_x, off_y) = if self.stage.data.background_type == BackgroundType::Tiled {
                    (frame_x % (batch.width() as f32), frame_y % (batch.height() as f32))
                } else {
                    (
                        ((frame_x / 2.0 * scale).floor() / scale) % (batch.width() as f32),
                        ((frame_y / 2.0 * scale).floor() / scale) % (batch.height() as f32),
                    )
                };

                let count_x = state.canvas_size.0 as usize / batch.width() + 2;
                let count_y = state.canvas_size.1 as usize / batch.height() + 2;

                for y in 0..count_y {
                    for x in 0..count_x {
                        batch.add((x * batch.width()) as f32 - off_x, (y * batch.height()) as f32 - off_y);
                    }
                }
            }
            BackgroundType::Water => {
                graphics::clear(ctx, self.stage.data.background_color);
            }
            BackgroundType::Black => {
                graphics::clear(ctx, self.stage.data.background_color);
            }
            BackgroundType::Scrolling => {
                graphics::clear(ctx, self.stage.data.background_color);
            }
            BackgroundType::OutsideWind | BackgroundType::Outside | BackgroundType::OutsideUnknown => {
                graphics::clear(ctx, Color::from_rgb(0, 0, 0));

                let offset = (self.tick % 640) as i32;

                for x in (0..(state.canvas_size.0 as i32)).step_by(200) {
                    batch.add_rect(x as f32, 0.0, &Rect::new_size(0, 0, 200, 88));
                }

                batch.add_rect(state.canvas_size.0 - 320.0, 0.0, &Rect::new_size(0, 0, 320, 88));

                for x in ((-offset / 2)..(state.canvas_size.0 as i32)).step_by(320) {
                    batch.add_rect(x as f32, 88.0, &Rect::new_size(0, 88, 320, 35));
                }

                for x in ((-offset % 320)..(state.canvas_size.0 as i32)).step_by(320) {
                    batch.add_rect(x as f32, 123.0, &Rect::new_size(0, 123, 320, 23));
                }

                for x in ((-offset * 2)..(state.canvas_size.0 as i32)).step_by(320) {
                    batch.add_rect(x as f32, 146.0, &Rect::new_size(0, 146, 320, 30));
                }

                for x in ((-offset * 4)..(state.canvas_size.0 as i32)).step_by(320) {
                    batch.add_rect(x as f32, 176.0, &Rect::new_size(0, 176, 320, 64));
                }
            }
        }

        batch.draw(ctx)?;

        Ok(())
    }

    fn draw_npc_layer(&self, state: &mut SharedGameState, ctx: &mut Context, layer: NPCLayer) -> GameResult {
        for npc in self.npc_list.iter_alive() {
            if npc.layer != layer
                || npc.x < (self.frame.x - 128 * 0x200 - npc.display_bounds.width() as i32 * 0x200)
                || npc.x
                    > (self.frame.x
                        + 128 * 0x200
                        + (state.canvas_size.0 as i32 + npc.display_bounds.width() as i32) * 0x200)
                    && npc.y < (self.frame.y - 128 * 0x200 - npc.display_bounds.height() as i32 * 0x200)
                || npc.y
                    > (self.frame.y
                        + 128 * 0x200
                        + (state.canvas_size.1 as i32 + npc.display_bounds.height() as i32) * 0x200)
            {
                continue;
            }

            npc.draw(state, ctx, &self.frame)?;
        }

        Ok(())
    }

    fn draw_bullets(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "Bullet")?;
        let mut x: i32;
        let mut y: i32;
        let mut prev_x: i32;
        let mut prev_y: i32;

        for bullet in self.bullet_manager.bullets.iter() {
            match bullet.direction {
                Direction::Left => {
                    x = bullet.x - bullet.display_bounds.left as i32;
                    y = bullet.y - bullet.display_bounds.top as i32;
                    prev_x = bullet.prev_x - bullet.display_bounds.left as i32;
                    prev_y = bullet.prev_y - bullet.display_bounds.top as i32;
                }
                Direction::Up => {
                    x = bullet.x - bullet.display_bounds.top as i32;
                    y = bullet.y - bullet.display_bounds.left as i32;
                    prev_x = bullet.prev_x - bullet.display_bounds.top as i32;
                    prev_y = bullet.prev_y - bullet.display_bounds.left as i32;
                }
                Direction::Right => {
                    x = bullet.x - bullet.display_bounds.right as i32;
                    y = bullet.y - bullet.display_bounds.top as i32;
                    prev_x = bullet.prev_x - bullet.display_bounds.right as i32;
                    prev_y = bullet.prev_y - bullet.display_bounds.top as i32;
                }
                Direction::Bottom => {
                    x = bullet.x - bullet.display_bounds.top as i32;
                    y = bullet.y - bullet.display_bounds.right as i32;
                    prev_x = bullet.prev_x - bullet.display_bounds.top as i32;
                    prev_y = bullet.prev_y - bullet.display_bounds.right as i32;
                }
                Direction::FacingPlayer => unreachable!(),
            }

            batch.add_rect(
                interpolate_fix9_scale(prev_x - self.frame.prev_x, x - self.frame.x, state.frame_time),
                interpolate_fix9_scale(prev_y - self.frame.prev_y, y - self.frame.y, state.frame_time),
                &bullet.anim_rect,
            );
        }

        batch.draw(ctx)?;
        Ok(())
    }

    fn draw_carets(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "Caret")?;

        for caret in state.carets.iter() {
            batch.add_rect(
                interpolate_fix9_scale(
                    caret.prev_x - caret.offset_x - self.frame.prev_x,
                    caret.x - caret.offset_x - self.frame.x,
                    state.frame_time,
                ),
                interpolate_fix9_scale(
                    caret.prev_y - caret.offset_y - self.frame.prev_y,
                    caret.y - caret.offset_y - self.frame.y,
                    state.frame_time,
                ),
                &caret.anim_rect,
            );
        }

        batch.draw(ctx)?;
        Ok(())
    }

    fn draw_fade(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        match state.fade_state {
            FadeState::Visible => {
                return Ok(());
            }
            FadeState::Hidden => {
                let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "Fade")?;
                let mut rect = Rect::new(0, 0, 16, 16);
                let frame = 15;
                rect.left = frame * 16;
                rect.right = rect.left + 16;

                for x in 0..(state.canvas_size.0 as i32 / 16 + 1) {
                    for y in 0..(state.canvas_size.1 as i32 / 16 + 1) {
                        batch.add_rect(x as f32 * 16.0, y as f32 * 16.0, &rect);
                    }
                }

                batch.draw(ctx)?;
            }
            FadeState::FadeIn(tick, direction) | FadeState::FadeOut(tick, direction) => {
                let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "Fade")?;
                let mut rect = Rect::new(0, 0, 16, 16);

                match direction {
                    FadeDirection::Left | FadeDirection::Right => {
                        let mut frame = tick;

                        for x in 0..(state.canvas_size.0 as i32 / 16 + 1) {
                            if frame >= 15 {
                                frame = 15;
                            } else {
                                frame += 1;
                            }

                            if frame >= 0 {
                                rect.left = frame.abs() as u16 * 16;
                                rect.right = rect.left + 16;

                                for y in 0..(state.canvas_size.1 as i32 / 16 + 1) {
                                    if direction == FadeDirection::Left {
                                        batch.add_rect(state.canvas_size.0 - x as f32 * 16.0, y as f32 * 16.0, &rect);
                                    } else {
                                        batch.add_rect(x as f32 * 16.0, y as f32 * 16.0, &rect);
                                    }
                                }
                            }
                        }
                    }
                    FadeDirection::Up | FadeDirection::Down => {
                        let mut frame = tick;

                        for y in 0..(state.canvas_size.1 as i32 / 16 + 1) {
                            if frame >= 15 {
                                frame = 15;
                            } else {
                                frame += 1;
                            }

                            if frame >= 0 {
                                rect.left = frame.abs() as u16 * 16;
                                rect.right = rect.left + 16;

                                for x in 0..(state.canvas_size.0 as i32 / 16 + 1) {
                                    if direction == FadeDirection::Down {
                                        batch.add_rect(x as f32 * 16.0, y as f32 * 16.0, &rect);
                                    } else {
                                        batch.add_rect(x as f32 * 16.0, state.canvas_size.1 - y as f32 * 16.0, &rect);
                                    }
                                }
                            }
                        }
                    }
                    FadeDirection::Center => {
                        let center_x = (state.canvas_size.0 / 2.0 - 8.0) as i32;
                        let center_y = (state.canvas_size.1 / 2.0 - 8.0) as i32;
                        let mut start_frame = tick;

                        for x in 0..(center_x / 16 + 2) {
                            let mut frame = start_frame;

                            for y in 0..(center_y / 16 + 2) {
                                if frame >= 15 {
                                    frame = 15;
                                } else {
                                    frame += 1;
                                }

                                if frame >= 0 {
                                    rect.left = frame.abs() as u16 * 16;
                                    rect.right = rect.left + 16;

                                    batch.add_rect((center_x - x * 16) as f32, (center_y + y * 16) as f32, &rect);
                                    batch.add_rect((center_x - x * 16) as f32, (center_y - y * 16) as f32, &rect);
                                    batch.add_rect((center_x + x * 16) as f32, (center_y + y * 16) as f32, &rect);
                                    batch.add_rect((center_x + x * 16) as f32, (center_y - y * 16) as f32, &rect);
                                }
                            }

                            start_frame += 1;
                        }
                    }
                }

                batch.draw(ctx)?;
            }
        }

        Ok(())
    }

    fn draw_black_bars(&self, _state: &mut SharedGameState, _ctx: &mut Context) -> GameResult {
        Ok(())
    }

    fn draw_text_boxes(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        if !state.textscript_vm.flags.render() {
            return Ok(());
        }

        let (off_left, off_top, off_right, off_bottom) =
            crate::framework::graphics::screen_insets_scaled(ctx, state.scale);

        let center = ((state.canvas_size.0 - off_left - off_right) / 2.0).floor();
        let top_pos = if state.textscript_vm.flags.position_top() {
            32.0 + off_top
        } else {
            state.canvas_size.1 as f32 - off_bottom - 66.0
        };
        let left_pos = off_left + center - 122.0;

        {
            let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "TextBox")?;
            if state.textscript_vm.flags.background_visible() {
                batch.add_rect(left_pos, top_pos, &state.constants.textscript.textbox_rect_top);
                for i in 1..7 {
                    batch.add_rect(left_pos, top_pos + i as f32 * 8.0, &state.constants.textscript.textbox_rect_middle);
                }
                batch.add_rect(left_pos, top_pos + 56.0, &state.constants.textscript.textbox_rect_bottom);
            }

            if state.textscript_vm.item != 0 {
                batch.add_rect(
                    center - 40.0,
                    state.canvas_size.1 - off_bottom - 112.0,
                    &state.constants.textscript.get_item_top_left,
                );
                batch.add_rect(
                    center - 40.0,
                    state.canvas_size.1 - off_bottom - 96.0,
                    &state.constants.textscript.get_item_bottom_left,
                );
                batch.add_rect(
                    center + 32.0,
                    state.canvas_size.1 - off_bottom - 112.0,
                    &state.constants.textscript.get_item_top_right,
                );
                batch.add_rect(
                    center + 32.0,
                    state.canvas_size.1 - off_bottom - 104.0,
                    &state.constants.textscript.get_item_right,
                );
                batch.add_rect(
                    center + 32.0,
                    state.canvas_size.1 - off_bottom - 96.0,
                    &state.constants.textscript.get_item_right,
                );
                batch.add_rect(
                    center + 32.0,
                    state.canvas_size.1 - off_bottom - 88.0,
                    &state.constants.textscript.get_item_bottom_right,
                );
            }

            if let TextScriptExecutionState::WaitConfirmation(_, _, _, wait, selection) = state.textscript_vm.state {
                let pos_y = if wait > 14 {
                    state.canvas_size.1 - off_bottom - 96.0 - (wait as f32 + 2.0) * 4.0
                } else {
                    state.canvas_size.1 - off_bottom - 96.0
                };

                batch.add_rect(center + 56.0, pos_y, &state.constants.textscript.textbox_rect_yes_no);

                if wait == 0 {
                    let pos_x = if selection == ConfirmSelection::No { 41.0 } else { 0.0 };

                    batch.add_rect(
                        center + 51.0 + pos_x,
                        pos_y + 10.0,
                        &state.constants.textscript.textbox_rect_cursor,
                    );
                }
            }

            batch.draw(ctx)?;
        }

        if state.textscript_vm.face != 0 {
            let tex_name = if state.constants.textscript.animated_face_pics { SWITCH_FACE_TEX[0] } else { FACE_TEX };
            let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, tex_name)?;

            // switch version uses 1xxx flag to show a flipped version of face
            let flip = state.textscript_vm.face > 1000;
            // x1xx flag shows a talking animation
            let _talking = (state.textscript_vm.face % 1000) > 100;
            let face_num = state.textscript_vm.face % 100;

            batch.add_rect_flip(
                left_pos + 14.0,
                top_pos + 8.0,
                flip,
                false,
                &Rect::new_size((face_num as u16 % 6) * 48, (face_num as u16 / 6) * 48, 48, 48),
            );

            batch.draw(ctx)?;
        }

        if state.textscript_vm.item != 0 {
            let mut rect = Rect::new(0, 0, 0, 0);

            if state.textscript_vm.item < 1000 {
                let item_id = state.textscript_vm.item as u16;

                rect.left = (item_id % 16) * 16;
                rect.right = rect.left + 16;
                rect.top = (item_id / 16) * 16;
                rect.bottom = rect.top + 16;

                let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "ArmsImage")?;
                batch.add_rect((center - 12.0).floor(), state.canvas_size.1 - off_bottom - 104.0, &rect);
                batch.draw(ctx)?;
            } else {
                let item_id = state.textscript_vm.item as u16 - 1000;

                rect.left = (item_id % 8) * 32;
                rect.right = rect.left + 32;
                rect.top = (item_id / 8) * 16;
                rect.bottom = rect.top + 16;

                let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "ItemImage")?;
                batch.add_rect((center - 20.0).floor(), state.canvas_size.1 - off_bottom - 104.0, &rect);
                batch.draw(ctx)?;
            }
        }

        let text_offset = if state.textscript_vm.face == 0 { 0.0 } else { 56.0 };

        let lines = [&state.textscript_vm.line_1, &state.textscript_vm.line_2, &state.textscript_vm.line_3];

        for (idx, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                if state.constants.textscript.text_shadow {
                    state.font.draw_text_with_shadow(
                        line.iter().copied(),
                        left_pos + text_offset + 14.0,
                        top_pos + 10.0 + idx as f32 * 16.0,
                        &state.constants,
                        &mut state.texture_set,
                        ctx,
                    )?;
                } else {
                    state.font.draw_text(
                        line.iter().copied(),
                        left_pos + text_offset + 14.0,
                        top_pos + 10.0 + idx as f32 * 16.0,
                        &state.constants,
                        &mut state.texture_set,
                        ctx,
                    )?;
                }
            }
        }

        if let TextScriptExecutionState::WaitInput(_, _, tick) = state.textscript_vm.state {
            if tick > 10 {
                let (mut x, y) = match state.textscript_vm.current_line {
                    TextScriptLine::Line1 => (
                        state.font.text_width(state.textscript_vm.line_1.iter().copied(), &state.constants),
                        top_pos + 10.0,
                    ),
                    TextScriptLine::Line2 => (
                        state.font.text_width(state.textscript_vm.line_2.iter().copied(), &state.constants),
                        top_pos + 10.0 + 16.0,
                    ),
                    TextScriptLine::Line3 => (
                        state.font.text_width(state.textscript_vm.line_3.iter().copied(), &state.constants),
                        top_pos + 10.0 + 32.0,
                    ),
                };
                x += left_pos + text_offset + 14.0;

                draw_rect(
                    ctx,
                    Rect::new_size(
                        (x * state.scale) as isize,
                        (y * state.scale) as isize,
                        (5.0 * state.scale) as isize,
                        (state.font.line_height(&state.constants) * state.scale) as isize,
                    ),
                    Color::from_rgb(255, 255, 255),
                )?;
            }
        }

        Ok(())
    }

    fn draw_light(&self, x: f32, y: f32, size: f32, color: (u8, u8, u8), batch: &mut SizedBatch) {
        batch.add_rect_scaled_tinted(
            x - size * 32.0,
            y - size * 32.0,
            (color.0, color.1, color.2, 255),
            size,
            size,
            &Rect::new(0, 0, 64, 64),
        )
    }

    fn draw_light_raycast(
        &self,
        tile_size: TileSize,
        world_point_x: i32,
        world_point_y: i32,
        (br, bg, bb): (u8, u8, u8),
        att: u8,
        angle: Range<i32>,
        batch: &mut SizedBatch,
    ) {
        let px = world_point_x as f32 / 512.0;
        let py = world_point_y as f32 / 512.0;

        let fx2 = self.frame.x as f32 / 512.0;
        let fy2 = self.frame.y as f32 / 512.0;

        let ti = tile_size.as_int();
        let tf = tile_size.as_float();
        let tih = ti / 2;
        let tfq = tf / 4.0;

        'ray: for deg in angle {
            let d = deg as f32 * (std::f32::consts::PI / 180.0);
            let dx = d.cos() * -5.0;
            let dy = d.sin() * -5.0;
            let mut x = px;
            let mut y = py;
            let mut r = br;
            let mut g = bg;
            let mut b = bb;

            for i in 0..25 {
                x += dx;
                y += dy;

                const ARR: [(i32, i32); 4] = [(0, 0), (0, 1), (1, 0), (1, 1)];
                for (ox, oy) in ARR.iter() {
                    let bx = x as i32 / ti + *ox;
                    let by = y as i32 / ti + *oy;

                    let tile = self.stage.map.attrib[self.stage.tile_at(bx as usize, by as usize) as usize];

                    if ((tile == 0x62 || tile == 0x41 || tile == 0x43 || tile == 0x46)
                        && x >= (bx * ti - tih) as f32
                        && x <= (bx * ti + tih) as f32
                        && y >= (by * ti - tih) as f32
                        && y <= (by * ti + tih) as f32)
                        || ((tile == 0x50 || tile == 0x70)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y <= ((by as f32 * tf) - (x - bx as f32 * tf) / 2.0 + tfq)
                            && y >= (by * ti - tih) as f32)
                        || ((tile == 0x51 || tile == 0x71)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y <= ((by as f32 * tf) - (x - bx as f32 * tf) / 2.0 - tfq)
                            && y >= (by * ti - tih) as f32)
                        || ((tile == 0x52 || tile == 0x72)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y <= ((by as f32 * tf) + (x - bx as f32 * tf) / 2.0 - tfq)
                            && y >= (by * ti - tih) as f32)
                        || ((tile == 0x53 || tile == 0x73)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y <= ((by as f32 * tf) + (x - bx as f32 * tf) / 2.0 + tfq)
                            && y >= (by * ti - tih) as f32)
                        || ((tile == 0x54 || tile == 0x74)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y >= ((by as f32 * tf) + (x - bx as f32 * tf) / 2.0 - tfq)
                            && y <= (by * ti + tih) as f32)
                        || ((tile == 0x55 || tile == 0x75)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y >= ((by as f32 * tf) + (x - bx as f32 * tf) / 2.0 + tfq)
                            && y <= (by * ti + tih) as f32)
                        || ((tile == 0x56 || tile == 0x76)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y >= ((by as f32 * tf) - (x - bx as f32 * tf) / 2.0 + tfq)
                            && y <= (by * ti + tih) as f32)
                        || ((tile == 0x57 || tile == 0x77)
                            && x >= (bx * ti - tih) as f32
                            && x <= (bx * ti + tih) as f32
                            && y >= ((by as f32 * tf) - (x - bx as f32 * tf) / 2.0 - tfq)
                            && y <= (by * ti + tih) as f32)
                    {
                        continue 'ray;
                    }
                }

                r = r.saturating_sub(att);
                g = g.saturating_sub(att);
                b = b.saturating_sub(att);

                if r == 0 && g == 0 && b == 0 {
                    continue 'ray;
                }

                self.draw_light(x - fx2, y - fy2, 0.12 + i as f32 / 75.0, (r, g, b), batch);
            }
        }
    }

    fn draw_light_map(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        let canvas = state.lightmap_canvas.as_mut();
        if let None = canvas {
            return Ok(());
        }

        let canvas = canvas.unwrap();

        graphics::set_render_target(ctx, Some(canvas))?;
        graphics::set_blend_mode(ctx, BlendMode::Add)?;

        graphics::clear(ctx, Color::from_rgb(100, 100, 110));
        {
            let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "builtin/lightmap/spot")?;

            for (player, inv) in
                [(&self.player1, &self.inventory_player1), (&self.player2, &self.inventory_player2)].iter()
            {
                if player.cond.alive() && !player.cond.hidden() && inv.get_current_weapon().is_some() {
                    let range = match () {
                        _ if player.up => 60..120,
                        _ if player.down => 240..300,
                        _ if player.direction == Direction::Left => -30..30,
                        _ if player.direction == Direction::Right => 150..210,
                        _ => 0..1,
                    };

                    let (color, att) = match inv.get_current_weapon() {
                        Some(Weapon { wtype: WeaponType::Fireball, .. }) => ((200u8, 70u8, 10u8), 6),
                        Some(Weapon { wtype: WeaponType::Spur, .. }) => ((170u8, 170u8, 200u8), 7),
                        _ => ((150u8, 150u8, 150u8), 7),
                    };

                    self.draw_light_raycast(state.tile_size, player.x, player.y, color, att, range, batch);
                }
            }

            for bullet in self.bullet_manager.bullets.iter() {
                self.draw_light(
                    interpolate_fix9_scale(
                        bullet.prev_x - self.frame.prev_x,
                        bullet.x - self.frame.x,
                        state.frame_time,
                    ),
                    interpolate_fix9_scale(
                        bullet.prev_y - self.frame.prev_y,
                        bullet.y - self.frame.y,
                        state.frame_time,
                    ),
                    0.3,
                    (200, 200, 200),
                    batch,
                );
            }

            for caret in state.carets.iter() {
                match caret.ctype {
                    CaretType::ProjectileDissipation | CaretType::Shoot => {
                        self.draw_light(
                            interpolate_fix9_scale(
                                caret.prev_x - self.frame.prev_x,
                                caret.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                caret.prev_y - self.frame.prev_y,
                                caret.y - self.frame.y,
                                state.frame_time,
                            ),
                            1.0,
                            (200, 200, 200),
                            batch,
                        );
                    }
                    _ => {}
                }
            }

            for npc in self.npc_list.iter_alive() {
                if npc.cond.hidden()
                    || (npc.x < (self.frame.x - 128 * 0x200 - npc.display_bounds.width() as i32 * 0x200)
                        || npc.x
                            > (self.frame.x
                                + 128 * 0x200
                                + (state.canvas_size.0 as i32 + npc.display_bounds.width() as i32) * 0x200)
                            && npc.y < (self.frame.y - 128 * 0x200 - npc.display_bounds.height() as i32 * 0x200)
                        || npc.y
                            > (self.frame.y
                                + 128 * 0x200
                                + (state.canvas_size.1 as i32 + npc.display_bounds.height() as i32) * 0x200))
                {
                    continue;
                }

                match npc.npc_type {
                    1 => {
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            0.4,
                            (255, 255, 0),
                            batch,
                        );
                    }
                    4 if npc.direction == Direction::Up => self.draw_light(
                        interpolate_fix9_scale(npc.prev_x - self.frame.prev_x, npc.x - self.frame.x, state.frame_time),
                        interpolate_fix9_scale(npc.prev_y - self.frame.prev_y, npc.y - self.frame.y, state.frame_time),
                        1.0,
                        (200, 100, 0),
                        batch,
                    ),
                    7 => self.draw_light(
                        interpolate_fix9_scale(npc.prev_x - self.frame.prev_x, npc.x - self.frame.x, state.frame_time),
                        interpolate_fix9_scale(npc.prev_y - self.frame.prev_y, npc.y - self.frame.y, state.frame_time),
                        1.0,
                        (100, 100, 100),
                        batch,
                    ),
                    17 if npc.anim_num == 0 => {
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            2.0,
                            (160, 0, 0),
                            batch,
                        );
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            0.5,
                            (255, 0, 0),
                            batch,
                        );
                    }
                    20 if npc.direction == Direction::Right => {
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            2.0,
                            (0, 0, 150),
                            batch,
                        );

                        if npc.anim_num < 2 {
                            self.draw_light(
                                interpolate_fix9_scale(
                                    npc.prev_x - self.frame.prev_x,
                                    npc.x - self.frame.x,
                                    state.frame_time,
                                ),
                                interpolate_fix9_scale(
                                    npc.prev_y - self.frame.prev_y,
                                    npc.y - self.frame.y,
                                    state.frame_time,
                                ),
                                2.1,
                                (0, 0, 30),
                                batch,
                            );
                        }
                    }
                    22 if npc.action_num == 1 && npc.anim_num == 1 => self.draw_light(
                        interpolate_fix9_scale(npc.prev_x - self.frame.prev_x, npc.x - self.frame.x, state.frame_time),
                        interpolate_fix9_scale(npc.prev_y - self.frame.prev_y, npc.y - self.frame.y, state.frame_time),
                        3.0,
                        (0, 0, 255),
                        batch,
                    ),
                    32 | 87 | 211 => {
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            2.0,
                            (255, 30, 30),
                            batch,
                        );
                    }
                    38 => {
                        let flicker = (npc.anim_num ^ 5 & 3) as u8 * 15;
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            3.5,
                            (130 + flicker, 40 + flicker, 0),
                            batch,
                        );
                    }
                    70 => {
                        let flicker = 50 + npc.anim_num as u8 * 15;
                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            2.0,
                            (flicker, flicker, flicker),
                            batch,
                        );
                    }
                    75 | 77 => self.draw_light(
                        interpolate_fix9_scale(npc.prev_x - self.frame.prev_x, npc.x - self.frame.x, state.frame_time),
                        interpolate_fix9_scale(npc.prev_y - self.frame.prev_y, npc.y - self.frame.y, state.frame_time),
                        3.0,
                        (255, 100, 0),
                        batch,
                    ),
                    85 if npc.action_num == 1 => {
                        let (color, color2) = if npc.direction == Direction::Left {
                            ((0, 150, 100), (0, 50, 30))
                        } else {
                            ((150, 0, 0), (50, 0, 0))
                        };

                        self.draw_light(
                            interpolate_fix9_scale(
                                npc.prev_x - self.frame.prev_x,
                                npc.x - self.frame.x,
                                state.frame_time,
                            ),
                            interpolate_fix9_scale(
                                npc.prev_y - self.frame.prev_y,
                                npc.y - self.frame.y,
                                state.frame_time,
                            ),
                            1.5,
                            color,
                            batch,
                        );

                        if npc.anim_num < 2 {
                            self.draw_light(
                                interpolate_fix9_scale(
                                    npc.prev_x - self.frame.prev_x,
                                    npc.x - self.frame.x,
                                    state.frame_time,
                                ),
                                interpolate_fix9_scale(
                                    npc.prev_y - self.frame.prev_y,
                                    npc.y - self.frame.y,
                                    state.frame_time,
                                ) - 8.0,
                                2.1,
                                color2,
                                batch,
                            );
                        }
                    }
                    _ => {}
                }
            }

            batch.draw_filtered(FilterMode::Linear, ctx)?;
        }

        graphics::set_blend_mode(ctx, BlendMode::Multiply)?;
        graphics::set_render_target(ctx, None)?;

        let rect = Rect { left: 0.0, top: 0.0, right: state.screen_size.0, bottom: state.screen_size.1 };
        canvas.clear();
        canvas.add(SpriteBatchCommand::DrawRect(rect, rect));
        canvas.draw()?;

        graphics::set_blend_mode(ctx, BlendMode::Alpha)?;

        Ok(())
    }

    fn draw_tiles(&self, state: &mut SharedGameState, ctx: &mut Context, layer: TileLayer) -> GameResult {
        if state.tile_size == TileSize::Tile8x8 && layer == TileLayer::Snack {
            return Ok(());
        }

        let tex = match layer {
            TileLayer::Snack => "Npc/NpcSym",
            TileLayer::Background => &self.tex_tileset_name_bg,
            TileLayer::Middleground => &self.tex_tileset_name_mg,
            TileLayer::Foreground => &self.tex_tileset_name,
        };

        let (layer_offset, layer_width, layer_height, uses_layers) = if let Some(pxpack_data) = self.stage.data.pxpack_data.as_ref() {
            match layer {
                TileLayer::Background => (pxpack_data.offset_bg as usize, pxpack_data.size_bg.0, pxpack_data.size_bg.1, true),
                TileLayer::Middleground => (pxpack_data.offset_mg as usize, pxpack_data.size_mg.0, pxpack_data.size_mg.1, true),
                _ => (0, pxpack_data.size_fg.0, pxpack_data.size_fg.1, true),
            }
        } else {
            (0, self.stage.map.width, self.stage.map.height, false)
        };

        if !uses_layers && layer == TileLayer::Middleground {
            return Ok(());
        }

        let tile_size = state.tile_size.as_int();
        let tile_sizef = state.tile_size.as_float();
        let halft = tile_size / 2;
        let halftf = tile_sizef / 2.0;

        let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, tex)?;
        let mut rect = Rect::new(0, 0, tile_size as u16, tile_size as u16);
        let (mut frame_x, mut frame_y) = self.frame.xy_interpolated(state.frame_time);

        if let Some(pxpack_data) = self.stage.data.pxpack_data.as_ref() {
            let (fx, fy) = match layer {
                TileLayer::Background =>  pxpack_data.scroll_bg.transform_camera_pos(frame_x, frame_y),
                TileLayer::Middleground =>  pxpack_data.scroll_mg.transform_camera_pos(frame_x, frame_y),
                _ => pxpack_data.scroll_fg.transform_camera_pos(frame_x, frame_y),
            };

            frame_x = fx;
            frame_y = fy;
        }

        let tile_start_x = (frame_x as i32 / tile_size).clamp(0, layer_width as i32) as usize;
        let tile_start_y = (frame_y as i32 / tile_size).clamp(0, layer_height as i32) as usize;
        let tile_end_x = ((frame_x as i32 + 8 + state.canvas_size.0 as i32) / tile_size + 1)
            .clamp(0, layer_width as i32) as usize;
        let tile_end_y = ((frame_y as i32 + halft + state.canvas_size.1 as i32) / tile_size + 1)
            .clamp(0, layer_height as i32) as usize;

        if layer == TileLayer::Snack {
            rect = state.constants.world.snack_rect;
        }

        for y in tile_start_y..tile_end_y {
            for x in tile_start_x..tile_end_x {
                let tile = *self.stage.map.tiles.get((y * layer_width as usize) + x + layer_offset).unwrap();
                match layer {
                    _ if uses_layers => {
                        if tile == 0 { continue; }

                        let tile_size = tile_size as u16;
                        rect.left = (tile as u16 % 16) * tile_size;
                        rect.top = (tile as u16 / 16) * tile_size;
                        rect.right = rect.left + tile_size;
                        rect.bottom = rect.top + tile_size;
                    }
                    TileLayer::Background => {
                        if self.stage.map.attrib[tile as usize] >= 0x20 {
                            continue;
                        }

                        let tile_size = tile_size as u16;
                        rect.left = (tile as u16 % 16) * tile_size;
                        rect.top = (tile as u16 / 16) * tile_size;
                        rect.right = rect.left + tile_size;
                        rect.bottom = rect.top + tile_size;
                    }
                    TileLayer::Foreground => {
                        let attr = self.stage.map.attrib[tile as usize];

                        if attr < 0x40 || attr >= 0x80 || attr == 0x43 {
                            continue;
                        }

                        let tile_size = tile_size as u16;
                        rect.left = (tile as u16 % 16) * tile_size;
                        rect.top = (tile as u16 / 16) * tile_size;
                        rect.right = rect.left + tile_size;
                        rect.bottom = rect.top + tile_size;
                    }
                    TileLayer::Snack => {
                        if self.stage.map.attrib[tile as usize] != 0x43 {
                            continue;
                        }
                    }
                    _ => {}
                }

                batch.add_rect(
                    (x as f32 * tile_sizef - halftf) - frame_x,
                    (y as f32 * tile_sizef - halftf) - frame_y,
                    &rect,
                );
            }
        }

        batch.draw(ctx)?;

        Ok(())
    }

    fn tick_npc_bullet_collissions(&mut self, state: &mut SharedGameState) {
        for npc in self.npc_list.iter_alive() {
            if npc.npc_flags.shootable() && npc.npc_flags.interactable() {
                continue;
            }

            for bullet in self.bullet_manager.bullets.iter_mut() {
                if !bullet.cond.alive() || bullet.damage < 0 {
                    continue;
                }

                if !npc.collides_with_bullet(bullet) {
                    continue;
                }

                if npc.npc_flags.shootable() {
                    npc.life = (npc.life as i32).saturating_sub(bullet.damage as i32).clamp(0, u16::MAX as i32) as u16;

                    if npc.life == 0 {
                        if npc.npc_flags.show_damage() {
                            npc.popup.add_value(-bullet.damage);
                        }

                        if self.player1.cond.alive() && npc.npc_flags.event_when_killed() {
                            state.control_flags.set_tick_world(true);
                            state.control_flags.set_interactions_disabled(true);
                            state.textscript_vm.start_script(npc.event_num);
                        } else {
                            npc.cond.set_explode_die(true);
                        }
                    } else {
                        if npc.shock < 14 {
                            if let Some(table_entry) = state.npc_table.get_entry(npc.npc_type) {
                                state.sound_manager.play_sfx(table_entry.hurt_sound);
                            }

                            npc.shock = 16;

                            for _ in 0..3 {
                                state.create_caret(
                                    (bullet.x + npc.x) / 2,
                                    (bullet.y + npc.y) / 2,
                                    CaretType::HurtParticles,
                                    Direction::Left,
                                );
                            }
                        }

                        if npc.npc_flags.show_damage() {
                            npc.popup.add_value(-bullet.damage);
                        }
                    }
                } else if !bullet.weapon_flags.flag_x10()
                    && bullet.btype != 13
                    && bullet.btype != 14
                    && bullet.btype != 15
                    && bullet.btype != 28
                    && bullet.btype != 29
                    && bullet.btype != 30
                {
                    state.create_caret(
                        (bullet.x + npc.x) / 2,
                        (bullet.y + npc.y) / 2,
                        CaretType::ProjectileDissipation,
                        Direction::Right,
                    );
                    state.sound_manager.play_sfx(31);
                    bullet.life = 0;
                    continue;
                }

                if bullet.life > 0 {
                    bullet.life -= 1;
                }
            }

            if npc.cond.explode_die() {
                let can_drop_missile = [&self.inventory_player1, &self.inventory_player2].iter().any(|inv| {
                    inv.has_weapon(WeaponType::MissileLauncher) || inv.has_weapon(WeaponType::SuperMissileLauncher)
                });

                self.npc_list.kill_npc(npc.id as usize, !npc.cond.drs_novanish(), can_drop_missile, state);
            }
        }

        for i in 0..self.boss.parts.len() {
            let mut idx = i;
            let mut npc = unsafe { self.boss.parts.get_unchecked_mut(i) };
            if !npc.cond.alive() {
                continue;
            }

            for bullet in self.bullet_manager.bullets.iter_mut() {
                if !bullet.cond.alive() || bullet.damage < 0 {
                    continue;
                }

                let hit = (npc.npc_flags.shootable()
                    && (npc.x - npc.hit_bounds.right as i32) < (bullet.x + bullet.enemy_hit_width as i32)
                    && (npc.x + npc.hit_bounds.right as i32) > (bullet.x - bullet.enemy_hit_width as i32)
                    && (npc.y - npc.hit_bounds.top as i32) < (bullet.y + bullet.enemy_hit_height as i32)
                    && (npc.y + npc.hit_bounds.bottom as i32) > (bullet.y - bullet.enemy_hit_height as i32))
                    || (npc.npc_flags.invulnerable()
                        && (npc.x - npc.hit_bounds.right as i32) < (bullet.x + bullet.hit_bounds.right as i32)
                        && (npc.x + npc.hit_bounds.right as i32) > (bullet.x - bullet.hit_bounds.left as i32)
                        && (npc.y - npc.hit_bounds.top as i32) < (bullet.y + bullet.hit_bounds.bottom as i32)
                        && (npc.y + npc.hit_bounds.bottom as i32) > (bullet.y - bullet.hit_bounds.top as i32));

                if !hit {
                    continue;
                }

                if npc.npc_flags.shootable() {
                    if npc.cond.damage_boss() {
                        idx = 0;
                        npc = unsafe { self.boss.parts.get_unchecked_mut(0) };
                    }

                    npc.life = (npc.life as i32).saturating_sub(bullet.damage as i32).clamp(0, u16::MAX as i32) as u16;

                    if npc.life == 0 {
                        npc.life = npc.id;

                        if self.player1.cond.alive() && npc.npc_flags.event_when_killed() {
                            state.control_flags.set_tick_world(true);
                            state.control_flags.set_interactions_disabled(true);
                            state.textscript_vm.start_script(npc.event_num);
                        } else {
                            state.sound_manager.play_sfx(self.boss.death_sound[idx]);

                            let destroy_count = 4usize * (2usize).pow((npc.size as u32).saturating_sub(1));

                            self.npc_list.create_death_smoke(
                                npc.x,
                                npc.y,
                                npc.display_bounds.right as usize,
                                destroy_count,
                                state,
                                &npc.rng,
                            );
                            npc.cond.set_alive(false);
                        }
                    } else {
                        if npc.shock < 14 {
                            for _ in 0..3 {
                                state.create_caret(bullet.x, bullet.y, CaretType::HurtParticles, Direction::Left);
                            }
                            state.sound_manager.play_sfx(self.boss.hurt_sound[idx]);
                        }

                        npc.shock = 8;

                        npc = unsafe { self.boss.parts.get_unchecked_mut(0) };
                        npc.shock = 8;
                    }

                    bullet.life = bullet.life.saturating_sub(1);
                    if bullet.life < 1 {
                        bullet.cond.set_alive(false);
                    }
                } else if [13, 14, 15, 28, 29, 30].contains(&bullet.btype) {
                    bullet.life = bullet.life.saturating_sub(1);
                } else if !bullet.weapon_flags.flag_x10() {
                    state.create_caret(bullet.x, bullet.y, CaretType::ProjectileDissipation, Direction::Right);
                    state.sound_manager.play_sfx(31);
                    bullet.life = 0;
                    continue;
                }
            }
        }
    }

    fn tick_world(&mut self, state: &mut SharedGameState) -> GameResult {
        self.hud_player1.visible = self.player1.cond.alive();
        self.hud_player2.visible = self.player2.cond.alive();
        self.hud_player1.has_player2 = self.player2.cond.alive() && !self.player2.cond.hidden();
        self.hud_player2.has_player2 = self.player1.cond.alive() && !self.player1.cond.hidden();

        self.player1.current_weapon = {
            if let Some(weapon) = self.inventory_player1.get_current_weapon_mut() {
                weapon.wtype as u8
            } else {
                0
            }
        };
        self.player2.current_weapon = {
            if let Some(weapon) = self.inventory_player2.get_current_weapon_mut() {
                weapon.wtype as u8
            } else {
                0
            }
        };
        self.player1.tick(state, &self.npc_list)?;
        self.player2.tick(state, &self.npc_list)?;

        if self.player1.damage > 0 {
            let xp_loss = self.player1.damage * if self.player1.equip.has_arms_barrier() { 1 } else { 2 };
            match self.inventory_player1.take_xp(xp_loss, state) {
                TakeExperienceResult::LevelDown if self.player1.life > 0 => {
                    state.create_caret(self.player1.x, self.player1.y, CaretType::LevelUp, Direction::Right);
                }
                _ => {}
            }

            self.player1.damage = 0;
        }

        if self.player2.damage > 0 {
            let xp_loss = self.player2.damage * if self.player2.equip.has_arms_barrier() { 1 } else { 2 };
            match self.inventory_player2.take_xp(xp_loss, state) {
                TakeExperienceResult::LevelDown if self.player2.life > 0 => {
                    state.create_caret(self.player2.x, self.player2.y, CaretType::LevelUp, Direction::Right);
                }
                _ => {}
            }

            self.player2.damage = 0;
        }

        for npc in self.npc_list.iter_alive() {
            npc.tick(
                state,
                (
                    [&mut self.player1, &mut self.player2],
                    &self.npc_list,
                    &mut self.stage,
                    &self.bullet_manager,
                    &mut self.flash,
                ),
            )?;
        }
        self.boss.tick(
            state,
            (
                [&mut self.player1, &mut self.player2],
                &self.npc_list,
                &mut self.stage,
                &self.bullet_manager,
                &mut self.flash,
            ),
        )?;

        self.player1.tick_map_collisions(state, &self.npc_list, &mut self.stage);
        self.player2.tick_map_collisions(state, &self.npc_list, &mut self.stage);

        self.player1.tick_npc_collisions(
            TargetPlayer::Player1,
            state,
            &self.npc_list,
            &mut self.boss,
            &mut self.inventory_player1,
        );
        self.player2.tick_npc_collisions(
            TargetPlayer::Player2,
            state,
            &self.npc_list,
            &mut self.boss,
            &mut self.inventory_player2,
        );

        for npc in self.npc_list.iter_alive() {
            if !npc.npc_flags.ignore_solidity() {
                npc.tick_map_collisions(state, &self.npc_list, &mut self.stage);
            }
        }
        for npc in self.boss.parts.iter_mut() {
            if npc.cond.alive() && !npc.npc_flags.ignore_solidity() {
                npc.tick_map_collisions(state, &self.npc_list, &mut self.stage);
            }
        }

        self.tick_npc_bullet_collissions(state);

        self.bullet_manager.tick_bullets(state, [&self.player1, &self.player2], &self.npc_list, &mut self.stage);
        state.tick_carets();

        match self.frame.update_target {
            UpdateTarget::Player => {
                if self.player2.cond.alive()
                    && !self.player2.cond.hidden()
                    && (self.player1.x - self.player2.x).abs() < 240 * 0x200
                    && (self.player1.y - self.player2.y).abs() < 200 * 0x200
                {
                    self.frame.target_x = (self.player1.target_x * 2 + self.player2.target_x) / 3;
                    self.frame.target_y = (self.player1.target_y * 2 + self.player2.target_y) / 3;

                    self.frame.target_x = self.frame.target_x.clamp(self.player1.x - 0x8000, self.player1.x + 0x8000);
                    self.frame.target_y = self.frame.target_y.clamp(self.player1.y, self.player1.y);
                } else {
                    self.frame.target_x = self.player1.target_x;
                    self.frame.target_y = self.player1.target_y;
                }
            }
            UpdateTarget::NPC(npc_id) => {
                if let Some(npc) = self.npc_list.get_npc(npc_id as usize) {
                    if npc.cond.alive() {
                        self.frame.target_x = npc.x;
                        self.frame.target_y = npc.y;
                    }
                }
            }
            UpdateTarget::Boss(boss_id) => {
                if let Some(boss) = self.boss.parts.get(boss_id as usize) {
                    if boss.cond.alive() {
                        self.frame.target_x = boss.x;
                        self.frame.target_y = boss.y;
                    }
                }
            }
        }
        self.frame.update(state, &self.stage);

        if state.control_flags.control_enabled() {
            self.inventory_player1.tick_weapons(
                state,
                &mut self.player1,
                TargetPlayer::Player1,
                &mut self.bullet_manager,
            );
            self.inventory_player2.tick_weapons(
                state,
                &mut self.player2,
                TargetPlayer::Player2,
                &mut self.bullet_manager,
            );

            self.hud_player1.tick(state, (&self.player1, &mut self.inventory_player1))?;
            self.hud_player2.tick(state, (&self.player2, &mut self.inventory_player2))?;
            self.boss_life_bar.tick(state, (&self.npc_list, &self.boss))?;

            if self.player1.controller.trigger_inventory() {
                state.textscript_vm.set_mode(ScriptMode::Inventory);
                self.player1.cond.set_interacted(false);
            }
        }

        self.water_renderer.tick(state, &self.player1)?;

        if self.map_name_counter > 0 {
            self.map_name_counter -= 1;
        }

        Ok(())
    }

    fn draw_debug_object(&self, entity: &dyn PhysicalEntity, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        if entity.x() < (self.frame.x - 128 - entity.display_bounds().width() as i32 * 0x200)
            || entity.x() > (self.frame.x + 128 + (state.canvas_size.0 as i32 + entity.display_bounds().width() as i32) * 0x200)
            && entity.y() < (self.frame.y - 128 - entity.display_bounds().height() as i32 * 0x200)
            || entity.y() > (self.frame.y + 128 + (state.canvas_size.1 as i32 + entity.display_bounds().height() as i32) * 0x200)
        {
            return Ok(());
        }

        {
            let hit_rect_size = entity.hit_rect_size().clamp(1, 4);
            let hit_rect_size = if state.tile_size == TileSize::Tile8x8 {
                4 * hit_rect_size * hit_rect_size
            } else {
                hit_rect_size * hit_rect_size
            };

            let tile_size = state.tile_size.as_int() * 0x200;
            let x = (entity.x() + entity.offset_x()) / tile_size;
            let y = (entity.y() + entity.offset_y()) / tile_size;

            let batch = state.texture_set.get_or_load_batch(ctx, &state.constants, "Caret")?;

            const CARET_RECT: Rect<u16> = Rect { left:2, top: 74, right: 6, bottom: 78};
            const CARET2_RECT: Rect<u16> = Rect {  left: 65, top: 9, right: 71, bottom: 15 };

            for (idx, &(ox, oy)) in OFFSETS.iter().enumerate() {
                if idx == hit_rect_size {
                    break;
                }

                batch.add_rect(
                    ((x + ox) * tile_size - self.frame.x) as f32 / 512.0 - 2.0,
                    ((y + oy) * tile_size - self.frame.y) as f32 / 512.0 - 2.0,
                    &CARET_RECT,
                );
            }

            batch.add_rect(
                (entity.x() - self.frame.x) as f32 / 512.0 - 3.0,
                (entity.y() - self.frame.y) as f32 / 512.0 - 3.0,
                &CARET2_RECT,
            );

            batch.draw(ctx)?;
        }

        Ok(())
    }

    fn draw_debug_npc(&self, npc: &NPC, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        self.draw_debug_object(npc, state, ctx)?;

        let text = format!("{}:{}:{}", npc.id, npc.npc_type, npc.action_num);
        state.font.draw_colored_text_scaled(
            text.chars(),
            ((npc.x - self.frame.x) / 0x200) as f32,
            ((npc.y - self.frame.y) / 0x200) as f32,
            0.5,
            ((npc.id & 0xf0) as u8, (npc.cond.0 >> 8) as u8, (npc.id & 0x0f << 4) as u8, 255),
            &state.constants,
            &mut state.texture_set,
            ctx,
        )?;

        Ok(())
    }

    fn draw_debug_outlines(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        for npc in self.npc_list.iter_alive() {
            self.draw_debug_npc(npc, state, ctx)?;
        }

        for boss in self.boss.parts.iter().filter(|n| n.cond.alive()) {
            self.draw_debug_npc(boss, state, ctx)?;
        }

        self.draw_debug_object(&self.player1, state, ctx)?;

        Ok(())
    }
}

impl Scene for GameScene {
    fn init(&mut self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        let seed = (self.player1.max_life as i32)
            .wrapping_add(self.player1.x as i32)
            .wrapping_add(self.player1.y as i32)
            .wrapping_add(self.stage_id as i32)
            .rotate_right(7);
        state.game_rng = XorShift::new(seed);
        state.textscript_vm.set_scene_script(self.stage.load_text_script(&state.base_path, &state.constants, ctx)?);
        state.textscript_vm.suspend = false;
        state.tile_size = self.stage.map.tile_size;
        #[cfg(feature = "scripting")]
            state.lua.set_game_scene(self as *mut _);

        self.player1.controller = state.settings.create_player1_controller();
        self.player2.controller = state.settings.create_player2_controller();

        let npcs = self.stage.load_npcs(&state.base_path, ctx)?;
        for npc_data in npcs.iter() {
            log::info!("creating npc: {:?}", npc_data);

            let mut npc = NPC::create_from_data(npc_data, &state.npc_table, state.tile_size);
            if npc.npc_flags.appear_when_flag_set() {
                if state.get_flag(npc_data.flag_num as _) {
                    npc.cond.set_alive(true);
                }
            } else if npc.npc_flags.hide_unless_flag_set() {
                if !state.get_flag(npc_data.flag_num as _) {
                    npc.cond.set_alive(true);
                }
            } else {
                npc.cond.set_alive(true);
            }

            self.npc_list.spawn_at_slot(npc_data.id, npc)?;
        }

        state.npc_table.tileset_name = self.tex_tileset_name.to_owned();
        state.npc_table.tex_npc1_name = ["Npc/", &self.stage.data.npc1.filename()].join("");
        state.npc_table.tex_npc2_name = ["Npc/", &self.stage.data.npc2.filename()].join("");

        /*if state.constants.is_cs_plus {
            match state.season {
                Season::Halloween => self.player1.appearance = PlayerAppearance::HalloweenQuote,
                Season::Christmas => self.player1.appearance = PlayerAppearance::ReindeerQuote,
                _ => {}
            }
        }*/

        self.boss.boss_type = self.stage.data.boss_no as u16;
        self.player1.target_x = self.player1.x;
        self.player1.target_y = self.player1.y;
        self.player1.camera_target_x = 0;
        self.player1.camera_target_y = 0;
        self.player2.target_x = self.player2.x;
        self.player2.target_y = self.player2.y;
        self.player2.camera_target_x = 0;
        self.player2.camera_target_y = 0;
        self.frame.target_x = self.player1.x;
        self.frame.target_y = self.player1.y;
        self.frame.immediate_update(state, &self.stage);

        Ok(())
    }

    fn tick(&mut self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        self.player1.controller.update(state, ctx)?;
        self.player1.controller.update_trigger();
        self.player2.controller.update(state, ctx)?;
        self.player2.controller.update_trigger();

        state.touch_controls.control_type =
            if state.control_flags.control_enabled() { TouchControlType::Controls } else { TouchControlType::None };

        if state.settings.touch_controls {
            state.touch_controls.interact_icon = false;
        }

        if self.intro_mode {
            state.touch_controls.control_type = TouchControlType::Dialog;

            if let TextScriptExecutionState::WaitTicks(_, _, 9999) = state.textscript_vm.state {
                state.next_scene = Some(Box::new(TitleScene::new()));
            }

            if self.player1.controller.trigger_menu_ok() {
                state.next_scene = Some(Box::new(TitleScene::new()));
            }
        }

        match state.textscript_vm.state {
            TextScriptExecutionState::Running(_, _)
            | TextScriptExecutionState::WaitTicks(_, _, _)
            | TextScriptExecutionState::WaitInput(_, _, _)
            | TextScriptExecutionState::Msg(_, _, _, _)
                if !state.control_flags.control_enabled() && !state.textscript_vm.flags.cutscene_skip() =>
            {
                if self.player1.controller.inventory() {
                    self.skip_counter += 1;
                    if self.skip_counter >= CUTSCENE_SKIP_WAIT {
                        state.textscript_vm.flags.set_cutscene_skip(true);
                    }
                } else if self.skip_counter > 0 {
                    self.skip_counter -= 1;
                }
            }
            _ => {
                self.skip_counter = 0;
            }
        }

        let mut ticks = 1;
        if state.textscript_vm.mode == ScriptMode::Map && state.textscript_vm.flags.cutscene_skip() {
            ticks = 4;
        }

        for _ in 0..ticks {
            match state.textscript_vm.mode {
                ScriptMode::Map if state.control_flags.tick_world() => self.tick_world(state)?,
                ScriptMode::StageSelect => self.stage_select.tick(state, (ctx, &self.player1, &self.player2))?,
                ScriptMode::Inventory => {
                    self.inventory_ui.tick(state, (ctx, &mut self.player1, &mut self.inventory_player1))?
                }
                _ => {}
            }

            match state.fade_state {
                FadeState::FadeOut(tick, direction) if tick < 15 => {
                    state.fade_state = FadeState::FadeOut(tick + 1, direction);
                }
                FadeState::FadeOut(tick, _) if tick == 15 => {
                    state.fade_state = FadeState::Hidden;
                }
                FadeState::FadeIn(tick, direction) if tick > -15 => {
                    state.fade_state = FadeState::FadeIn(tick - 1, direction);
                }
                FadeState::FadeIn(tick, _) if tick == -15 => {
                    state.fade_state = FadeState::Visible;
                }
                _ => {}
            }

            self.flash.tick(state, ())?;
            TextScriptVM::run(state, self, ctx)?;

            #[cfg(feature = "scripting")]
            state.lua.scene_tick();

            if state.control_flags.control_enabled() {
                self.tick = self.tick.wrapping_add(1);
            }
        }

        Ok(())
    }

    fn draw_tick(&mut self, state: &mut SharedGameState) -> GameResult {
        self.frame.prev_x = self.frame.x;
        self.frame.prev_y = self.frame.y;
        self.player1.prev_x = self.player1.x;
        self.player1.prev_y = self.player1.y;
        self.player1.popup.prev_x = self.player1.popup.x;
        self.player1.popup.prev_y = self.player1.popup.y;
        self.player2.prev_x = self.player2.x;
        self.player2.prev_y = self.player2.y;
        self.player2.popup.prev_x = self.player2.popup.x;
        self.player2.popup.prev_y = self.player2.popup.y;

        for npc in self.npc_list.iter_alive() {
            npc.prev_x = npc.x;
            npc.prev_y = npc.y;
            npc.popup.prev_x = npc.prev_x;
            npc.popup.prev_y = npc.prev_y;
        }

        for npc in self.boss.parts.iter_mut() {
            if npc.cond.alive() {
                npc.prev_x = npc.x;
                npc.prev_y = npc.y;
                npc.popup.prev_x = npc.prev_x;
                npc.popup.prev_y = npc.prev_y;
            }
        }

        for bullet in self.bullet_manager.bullets.iter_mut() {
            if bullet.cond.alive() {
                bullet.prev_x = bullet.x;
                bullet.prev_y = bullet.y;
            }
        }

        for caret in state.carets.iter_mut() {
            if caret.cond.alive() {
                caret.prev_x = caret.x;
                caret.prev_y = caret.y;
            }
        }

        Ok(())
    }

    fn draw(&self, state: &mut SharedGameState, ctx: &mut Context) -> GameResult {
        //graphics::set_canvas(ctx, Some(&state.game_canvas));
        self.draw_background(state, ctx)?;
        self.draw_tiles(state, ctx, TileLayer::Background)?;
        self.draw_npc_layer(state, ctx, NPCLayer::Background)?;
        self.draw_tiles(state, ctx, TileLayer::Middleground)?;

        if state.settings.shader_effects
            && self.stage.data.background_type != BackgroundType::Black
            && self.stage.data.background_type != BackgroundType::Outside
            && self.stage.data.background_type != BackgroundType::OutsideWind
            && self.stage.data.background.name() != "bkBlack"
        {
            self.draw_light_map(state, ctx)?;
        }

        self.boss.draw(state, ctx, &self.frame)?;
        self.draw_npc_layer(state, ctx, NPCLayer::Middleground)?;
        self.draw_bullets(state, ctx)?;
        self.player2.draw(state, ctx, &self.frame)?;
        self.player1.draw(state, ctx, &self.frame)?;

        self.water_renderer.draw(state, ctx, &self.frame)?;
        self.draw_tiles(state, ctx, TileLayer::Foreground)?;
        self.draw_tiles(state, ctx, TileLayer::Snack)?;
        self.draw_carets(state, ctx)?;
        self.player1.popup.draw(state, ctx, &self.frame)?;
        self.player2.popup.draw(state, ctx, &self.frame)?;

        // if !self.intro_mode && state.settings.shader_effects
        //     && (self.stage.data.background_type == BackgroundType::Black
        //         || self.stage.data.background.name() == "bkBlack")
        // {
        //     self.draw_light_map(state, ctx)?;
        // }
        self.flash.draw(state, ctx, &self.frame)?;

        /*graphics::set_canvas(ctx, None);
        state.game_canvas.draw(ctx, DrawParam::new()
            .scale(Vector2::new(1.0 / state.scale, 1.0 / state.scale)))?;*/
        self.draw_black_bars(state, ctx)?;

        match state.textscript_vm.mode {
            ScriptMode::Map if state.control_flags.control_enabled() => {
                self.hud_player1.draw(state, ctx, &self.frame)?;
                self.hud_player2.draw(state, ctx, &self.frame)?;
                self.boss_life_bar.draw(state, ctx, &self.frame)?;

                if self.player2.cond.alive() && !self.player2.cond.hidden() {
                    let y = interpolate_fix9_scale(
                        self.player2.prev_y - self.frame.prev_y,
                        self.player2.y - self.frame.y,
                        state.frame_time,
                    );
                    let y = y.clamp(8.0, state.canvas_size.1 - 8.0 - state.font.line_height(&state.constants));

                    if self.player2.x + 0x1000 < self.frame.x {
                        state.font.draw_colored_text(
                            P2_LEFT_TEXT.chars(),
                            9.0,
                            y + 1.0,
                            (0, 0, 130, 255),
                            &state.constants,
                            &mut state.texture_set,
                            ctx,
                        )?;

                        state.font.draw_colored_text(
                            P2_LEFT_TEXT.chars(),
                            8.0,
                            y,
                            (96, 96, 255, 255),
                            &state.constants,
                            &mut state.texture_set,
                            ctx,
                        )?;
                    } else if self.player2.x - 0x1000 > self.frame.x + state.canvas_size.0 as i32 * 0x200 {
                        let width = state.font.text_width(P2_RIGHT_TEXT.chars(), &state.constants);

                        state.font.draw_colored_text(
                            P2_RIGHT_TEXT.chars(),
                            state.canvas_size.0 - width - 8.0 + 1.0,
                            y + 1.0,
                            (0, 0, 130, 255),
                            &state.constants,
                            &mut state.texture_set,
                            ctx,
                        )?;

                        state.font.draw_colored_text(
                            P2_RIGHT_TEXT.chars(),
                            state.canvas_size.0 - width - 8.0,
                            y,
                            (96, 96, 255, 255),
                            &state.constants,
                            &mut state.texture_set,
                            ctx,
                        )?;
                    }
                }
            }
            ScriptMode::StageSelect => self.stage_select.draw(state, ctx, &self.frame)?,
            ScriptMode::Inventory => self.inventory_ui.draw(state, ctx, &self.frame)?,
            _ => {}
        }

        self.draw_fade(state, ctx)?;
        if state.textscript_vm.mode == ScriptMode::Map && self.map_name_counter > 0 {
            let map_name = if self.stage.data.name == "u" {
                state.constants.title.intro_text.chars()
            } else {
                self.stage.data.name.chars()
            };
            let width = state.font.text_width(map_name.clone(), &state.constants);

            state.font.draw_text_with_shadow(
                map_name,
                ((state.canvas_size.0 - width) / 2.0).floor(),
                80.0,
                &state.constants,
                &mut state.texture_set,
                ctx,
            )?;
        }

        self.draw_text_boxes(state, ctx)?;
        if self.skip_counter > 0 {
            let text = format!("Hold {:?} to skip the cutscene", state.settings.player1_key_map.inventory);
            let width = state.font.text_width(text.chars(), &state.constants);
            let pos_x = state.canvas_size.0 - width - 20.0;
            let pos_y = 0.0;
            let line_height = state.font.line_height(&state.constants);
            let w = (self.skip_counter as f32 / CUTSCENE_SKIP_WAIT as f32) * (width + 20.0) / 2.0;
            let mut rect = Rect::new_size(
                (pos_x * state.scale) as isize,
                (pos_y * state.scale) as isize,
                ((20.0 + width) * state.scale) as isize,
                ((20.0 + line_height) * state.scale) as isize,
            );

            draw_rect(ctx, rect, Color::from_rgb(0, 0, 32))?;

            rect.right = rect.left + (w * state.scale) as isize;
            draw_rect(ctx, rect, Color::from_rgb(160, 181, 222))?;

            rect.left = ((state.canvas_size.0 - w) * state.scale) as isize;
            rect.right = rect.left + (w * state.scale) as isize;
            draw_rect(ctx, rect, Color::from_rgb(160, 181, 222))?;

            state.font.draw_text_with_shadow(
                text.chars(),
                pos_x + 10.0,
                pos_y + 10.0,
                &state.constants,
                &mut state.texture_set,
                ctx,
            )?;
        }

        if state.settings.debug_outlines {
            self.draw_debug_outlines(state, ctx)?;
        }

        //draw_number(state.canvas_size.0 - 8.0, 8.0, timer::fps(ctx) as usize, Alignment::Right, state, ctx)?;
        Ok(())
    }

    fn debug_overlay_draw(
        &mut self,
        components: &mut Components,
        state: &mut SharedGameState,
        ctx: &mut Context,
        ui: &mut imgui::Ui,
    ) -> GameResult {
        components.live_debugger.run_ingame(self, state, ctx, ui)?;
        Ok(())
    }
}
