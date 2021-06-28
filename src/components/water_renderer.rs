use crate::common::{Color, Rect};
use crate::entity::GameEntity;
use crate::frame::Frame;
use crate::framework::backend::{BackendShader, VertexData};
use crate::framework::context::Context;
use crate::framework::error::GameResult;
use crate::framework::graphics;
use crate::map::WaterRegionType;
use crate::player::Player;
use crate::shared_game_state::SharedGameState;

const TENSION: f32 = 0.03;
const DAMPENING: f32 = 0.01;
const SPREAD: f32 = 0.02;

struct DynamicWaterColumn {
    target_height: f32,
    height: f32,
    speed: f32,
}

impl DynamicWaterColumn {
    pub fn new() -> DynamicWaterColumn {
        DynamicWaterColumn { target_height: 8.0, height: 8.0, speed: 0.0 }
    }

    pub fn tick(&mut self, dampening: f32, tension: f32) {
        self.speed += tension * (self.target_height - self.height) - self.speed * dampening;
        self.height += self.speed;
    }
}

pub struct DynamicWater {
    x: u16,
    y: u16,
    end_x: u16,
    columns: Vec<DynamicWaterColumn>,
}

impl DynamicWater {
    pub fn new(x: u16, y: u16, length: u16) -> DynamicWater {
        let mut columns = Vec::new();
        let count = length as usize * 8 + 1;

        for _ in 0..count {
            columns.push(DynamicWaterColumn::new());
        }

        DynamicWater { x, y, end_x: x + length, columns }
    }

    pub fn tick(&mut self) {
        for col in self.columns.iter_mut() {
            col.tick(DAMPENING, TENSION);
        }

        static mut l_deltas: Vec<f32> = Vec::new();
        static mut r_deltas: Vec<f32> = Vec::new();

        // we assume tick() is never called from other threads.
        unsafe {
            l_deltas.resize(self.columns.len(), 0.0);
            r_deltas.resize(self.columns.len(), 0.0);

            for _ in 0..2 {
                for i in 0..self.columns.len() {
                    if i > 0 {
                        l_deltas[i] = SPREAD * (self.columns[i].height - self.columns[i - 1].height);
                        self.columns[i - 1].speed += l_deltas[i];
                    }

                    if i < self.columns.len() - 1 {
                        r_deltas[i] = SPREAD * (self.columns[i].height - self.columns[i + 1].height);
                        self.columns[i + 1].speed += r_deltas[i];
                    }
                }

                for i in 0..self.columns.len() {
                    if i > 0 {
                        self.columns[i - 1].height += l_deltas[i];
                    }

                    if i < self.columns.len() - 1 {
                        self.columns[i + 1].height += r_deltas[i];
                    }
                }
            }
        }
    }
}

pub struct WaterRenderer {
    depth_regions: Vec<Rect<u16>>,
    surf_regions: Vec<Rect<u16>>,
    water_surfaces: Vec<DynamicWater>,
}

impl WaterRenderer {
    pub fn new() -> WaterRenderer {
        WaterRenderer { depth_regions: Vec::new(), surf_regions: Vec::new(), water_surfaces: Vec::new() }
    }

    pub fn initialize(&mut self, regions: Vec<(WaterRegionType, Rect<u16>)>) {
        for (reg_type, bounds) in regions {
            match reg_type {
                WaterRegionType::WaterLine => {
                    self.surf_regions.push(bounds);
                    self.water_surfaces.push(DynamicWater::new(bounds.left, bounds.top, bounds.width() + 1));
                }
                WaterRegionType::WaterDepth => {
                    self.depth_regions.push(bounds);
                }
            }
        }
    }
}

impl GameEntity<&Player> for WaterRenderer {
    fn tick(&mut self, _state: &mut SharedGameState, player: &Player) -> GameResult<()> {
        let player_x = player.x as f32 / 512.0 + 8.0;
        let player_y = player.y as f32 / 512.0 + 8.0;

        for surf in self.water_surfaces.iter_mut() {
            let line_x = surf.x as f32 * 16.0;
            let line_y = surf.y as f32 * 16.0;
            if (player.vel_y > 0x80 || player.vel_y < -0x80)
                && player_x > line_x
                && player_x < surf.end_x as f32 * 16.0
                && player_y > line_y - 5.0
                && player_y < line_y + 4.0
            {
                let col_idx_center = (((player_x - line_x) / 2.0) as i32).clamp(0, surf.columns.len() as i32);
                let col_idx_left = (col_idx_center - (player.hit_bounds.left as i32 / (8 * 0x200)))
                    .clamp(0, surf.columns.len() as i32) as usize;
                let col_idx_right = (col_idx_center + (player.hit_bounds.left as i32 / (8 * 0x200)))
                    .clamp(0, surf.columns.len() as i32) as usize;

                for col in surf.columns[col_idx_left..=col_idx_right].iter_mut() {
                    col.speed = player.vel_y as f32 / 512.0;
                }
            }

            surf.tick();
        }

        Ok(())
    }

    fn draw(&self, state: &mut SharedGameState, ctx: &mut Context, frame: &Frame) -> GameResult<()> {
        let mut out_rect = Rect::new(0, 0, 0, 0);
        let (o_x, o_y) = frame.xy_interpolated(state.frame_time);
        let water_color_top = Color::from_rgba(102, 153, 204, 150);
        let water_color = Color::from_rgba(102, 153, 204, 75);

        for region in self.depth_regions.iter() {
            out_rect.left = ((region.left as f32 * 16.0 - o_x - 8.0) * state.scale) as isize;
            out_rect.top = ((region.top as f32 * 16.0 - o_y - 8.0) * state.scale) as isize;
            out_rect.right = ((region.right as f32 * 16.0 - o_x + 8.0) * state.scale) as isize;
            out_rect.bottom = ((region.bottom as f32 * 16.0 - o_y + 8.0) * state.scale) as isize;
            graphics::draw_rect(ctx, out_rect, water_color)?;
        }

        if !graphics::supports_vertex_draw(ctx)? {
            for region in self.surf_regions.iter() {
                out_rect.left = ((region.left as f32 * 16.0 - o_x - 8.0) * state.scale) as isize;
                out_rect.top = ((region.top as f32 * 16.0 - o_y - 5.0) * state.scale) as isize;
                out_rect.right = ((region.right as f32 * 16.0 - o_x + 8.0) * state.scale) as isize;
                out_rect.bottom = ((region.bottom as f32 * 16.0 - o_y + 8.0) * state.scale) as isize;
                graphics::draw_rect(ctx, out_rect, water_color)?;
            }

            return Ok(());
        }

        let uv = (0.0, 0.0);
        let color_top_rgba = water_color_top.to_rgba();
        let color_mid_rgba = water_color.to_rgba();
        let color_btm_rgba = water_color.to_rgba();

        for surf in self.water_surfaces.iter() {
            let pos_x = surf.x as f32 * 16.0;
            let pos_y = surf.y as f32 * 16.0;

            if (pos_x - o_x - 16.0) > state.canvas_size.0
                || (pos_x - o_x + 16.0 + surf.end_x as f32 * 16.0) < 0.0
                || (pos_y - o_y - 16.0) > state.canvas_size.1
                || (pos_y - o_y + 16.0) < 0.0
            {
                continue;
            }

            let mut vertices = vec![];
            vertices.reserve(12 * surf.columns.len());

            let bottom = (pos_y - o_y + 8.0) * state.scale;
            for i in 1..surf.columns.len() {
                let x_right = (pos_x - 8.0 - o_x + i as f32 * 2.0) * state.scale;
                let x_left = x_right - 2.0 * state.scale;
                let top_left = (pos_y - o_y - 13.0 + surf.columns[i - 1].height) * state.scale;
                let top_right = (pos_y - o_y - 13.0 + surf.columns[i].height) * state.scale;
                let middle_left = top_left + 6.0 * state.scale;
                let middle_right = top_left + 6.0 * state.scale;

                vertices.push(VertexData { position: (x_left, middle_left), uv, color: color_mid_rgba });
                vertices.push(VertexData { position: (x_left, top_left), uv, color: color_top_rgba });
                vertices.push(VertexData { position: (x_right, top_right), uv, color: color_top_rgba });
                vertices.push(VertexData { position: (x_left, middle_left), uv, color: color_mid_rgba });
                vertices.push(VertexData { position: (x_right, top_right), uv, color: color_top_rgba });
                vertices.push(VertexData { position: (x_right, middle_right), uv, color: color_mid_rgba });

                vertices.push(VertexData { position: (x_left, bottom), uv, color: color_btm_rgba });
                vertices.push(VertexData { position: (x_left, middle_left), uv, color: color_mid_rgba });
                vertices.push(VertexData { position: (x_right, middle_right), uv, color: color_mid_rgba });
                vertices.push(VertexData { position: (x_left, bottom), uv, color: color_btm_rgba });
                vertices.push(VertexData { position: (x_right, middle_right), uv, color: color_mid_rgba });
                vertices.push(VertexData { position: (x_right, bottom), uv, color: color_btm_rgba });
            }

            graphics::draw_triangle_list(ctx, vertices, None, BackendShader::Fill);
        }

        Ok(())
    }
}
