use crate::bitfield;
use crate::common::{Color, Direction, Rect};

pub mod basic;
pub mod pxchar;

bitfield! {
  #[derive(Clone, Copy)]
  pub struct PlayerSkinFlags(u16);
  impl Debug;

  pub supports_color, set_supports_color: 0;
}

#[derive(Clone, Copy, PartialEq, Eq)]
/// Represents a player animation state.
pub enum PlayerAnimationState {
    Idle,
    Walking,
    WalkingUp,
    LookingUp,
    Examining,
    Sitting,
    Collapsed,
    Jumping,
    Falling,
    FallingLookingUp,
    FallingLookingDown,
    FallingUpsideDown,
}

/// Represents an alternative appearance of player eg. wearing a Mimiga Mask
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PlayerAppearanceState {
    Default,
    MimigaMask,
    /// Freeware hacks add a <MIMxxxx TSC instruction that sets an offset in player spritesheet to
    /// have more than two player appearances.
    Custom(u8),
}

/// Represents an interface for implementations of player skin providers.
pub trait PlayerSkin: PlayerSkinClone {
    /// Returns animation frame bounds for specified state.
    fn animation_frame_for(&self, state: PlayerAnimationState, direction: Direction, tick: u16) -> Rect<u16>;

    /// Returns animation frame bounds for current state.
    fn animation_frame(&self) -> Rect<u16>;

    /// Updates internal animation counters, must be called every game tick.
    fn tick(&mut self);

    /// Sets the current animation state.
    fn set_state(&mut self, state: PlayerAnimationState);

    /// Returns current animation state.
    fn get_state(&self) -> PlayerAnimationState;

    /// Sets the current appearance of skin.
    fn set_appearance(&mut self, appearance: PlayerAppearanceState);

    /// Returns current appearance of skin.
    fn get_appearance(&mut self) -> PlayerAppearanceState;

    /// Sets the current color of skin.
    fn set_color(&mut self, color: Color);

    /// Returns the current color of skin.
    fn get_color(&self) -> Color;

    /// Sets the current direction;
    fn set_direction(&mut self, direction: Direction);

    /// Returns the current direction;
    fn get_direction(&self) -> Direction;

    /// Returns the name of skin spritesheet texture.
    fn get_skin_texture_name(&self) -> &str;

    /// Returns the name of skin color mask texture.
    fn get_mask_texture_name(&self) -> &str;

    /// Returns hit bounds of skin.
    fn get_hit_bounds(&self) -> Rect<u32>;

    /// Returns display bounds of skin.
    fn get_display_bounds(&self) -> Rect<u32>;

    fn get_gun_offset(&self) -> (i32, i32);
}

pub trait PlayerSkinClone {
    fn clone_box(&self) -> Box<dyn PlayerSkin>;
}

impl<T: 'static + PlayerSkin + Clone> PlayerSkinClone for T {
    fn clone_box(&self) -> Box<dyn PlayerSkin> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn PlayerSkin> {
    fn clone(&self) -> Box<dyn PlayerSkin> {
        self.clone_box()
    }
}
