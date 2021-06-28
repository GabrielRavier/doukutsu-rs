use crate::player::{Player, TargetPlayer};
use crate::shared_game_state::SharedGameState;
use crate::weapon::bullet::BulletManager;
use crate::weapon::Weapon;

impl Weapon {
    pub(in crate::weapon) fn tick_missile_launcher(
        &mut self,
        _player: &mut Player,
        _player_id: TargetPlayer,
        _bullet_manager: &mut BulletManager,
        _state: &mut SharedGameState,
    ) {}
}
