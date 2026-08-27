#!/usr/bin/env python3
"""Display strings for every generated custom-options texture.

Data-only module consumed by gen_option_labels.py — no Pillow, no rendering.
Every entry carries per-language text keyed by language code ("en"/"ja"/"ko");
AVAILABLE_LANGS lists the languages with complete coverage (the generator
loops these by default). Japanese and Korean content lands in a later step of
the localization feature; see the design document at
.agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md
(Appendix A holds the approved translation tables).
"""

from typing import NamedTuple, Optional, Sequence

# Languages with complete string coverage. The generator loops these unless
# restricted with --lang.
AVAILABLE_LANGS = ["en", "ja", "ko"]

# Layout kinds for PreviewSpec.layout / TemplateSpec (see gen_option_labels):
#   SPLIT - dotted divider down the middle; copy confined to the left half.
#   WIDE  - no divider; copy spans the whole panel.
SPLIT = "split"
WIDE = "wide"


class PreviewSpec(NamedTuple):
    """One preview panel to generate (seop_image_<option>[_<value>].png).

    ``paragraphs`` maps language code -> list of body-copy paragraphs.
    ``art``/``art_pos`` name a scripts/templates/ image composited at the
    given (x, y) — the baked right-side art of the SPLIT panels taken over
    from the hand-authored originals (populated in the take-over step;
    None for text-only panels).
    """

    option: str
    value: Optional[str]
    layout: str
    paragraphs: dict
    art: Optional[str] = None
    art_pos: Optional[tuple] = None


class TemplateSpec(NamedTuple):
    """One customize preview-chrome template
    (seop_image_customize_<...>_TEMPLATE.png).

    ``markers`` is a list of (x, y, w, h, (r, g, b, a)) solid marker
    rectangles — byte-identical across languages (the DLL parses them for
    art placement; geometry measured off the shipped hand-authored
    templates). ``lines`` maps language code -> the EXPLICIT text lines
    (the hand-authored originals use hand-chosen breaks narrower than the
    preview wrap width, so template text is pre-broken rather than
    auto-wrapped; first line indented, 16px pitch from baseline 25 — same
    grid as the preview panels).
    """

    option: str
    markers: Sequence[tuple]
    lines: dict


# ── Left-side row labels (seop_item_<id>) ────────────────────────────────
LABELS = {
    "autoplay": {
        "en": 'AUTOPLAY',
        "ja": 'オートプレイ',
        "ko": '자동 플레이',
    },
    "premium_free": {
        "en": 'PREMIUM FREE',
        "ja": 'プレミアムフリー',
        "ko": '프리미엄 프리',
    },
    "center_arrows_1p": {
        "en": 'CENTER ARROWS (1P ONLY)',
        "ja": '矢印を中央に表示 (1P専用)',
        "ko": '화살표 중앙 표시 (1P 전용)',
    },
    "assist_tick": {
        "en": 'ASSIST TICK',
        "ja": 'アシストティック',
        "ko": '어시스트 틱',
    },
    "assist_tick_volume": {
        "en": 'TICK EFFECT VOLUME',
        "ja": 'ティック音量',
        "ko": '틱 효과음 볼륨',
    },
    "announcer_mute": {
        "en": 'ANNOUNCER MUTE',
        "ja": 'アナウンスミュート',
        "ko": '아나운서 음소거',
    },
    "skip_results_fast_exit": {
        "en": 'SKIP RESULTS ON FAST EXIT',
        "ja": 'クイック退出時のリザルトスキップ',
        "ko": '빠른 종료 시 결과 화면 생략',
    },
    "adjust_song_offset": {
        "en": 'ADJUST OFFSET FOR CURRENT SONG',
        "ja": '選択中の曲のオフセット調整',
        "ko": '현재 곡 오프셋 조정',
    },
    "current_song_offset": {
        "en": 'CURRENT SONG OFFSET',
        "ja": '選択中の曲のオフセット',
        "ko": '현재 곡 오프셋',
    },
    "timing_stats": {
        "en": 'REALTIME GAMEPLAY STATISTICS',
        "ja": 'リアルタイムプレイ統計',
        "ko": '실시간 플레이 통계',
    },
    "pacemaker_to_mserror": {
        "en": 'PACEMAKER -> MS ERROR',
        "ja": 'ペースメーカー→ms誤差',
        "ko": '페이스메이커 → ms 오차',
    },
    "pacemaker_threshold": {
        "en": 'WHITE THRESHOLD',
        "ja": '白表示のしきい値',
        "ko": '흰색 표시 임계값',
    },
    "step_data_export": {
        "en": 'EXPORT STEP DATA (CSV)',
        "ja": 'ステップデータ出力 (CSV)',
        "ko": '스텝 데이터 내보내기 (CSV)',
    },
    "customize_appeal_board": {
        "en": 'APPEAL BOARD',
        "ja": 'アピールボード',
        "ko": '어필 보드',
    },
    "customize_background": {
        "en": 'BACKGROUND',
        "ja": '背景',
        "ko": '배경',
    },
    "customize_background_gameplay": {
        "en": 'BACKGROUND (GAMEPLAY)',
        "ja": '背景 (プレイ中)',
        "ko": '배경 (게임 플레이)',
    },
    "customize_character_p1": {
        "en": 'CHARACTER (P1)',
        "ja": 'キャラクター (P1)',
        "ko": '캐릭터 (P1)',
    },
    "customize_character_p2": {
        "en": 'CHARACTER (P2)',
        "ja": 'キャラクター (P2)',
        "ko": '캐릭터 (P2)',
    },
    "customize_lane_single": {
        "en": 'LANE (SINGLE)',
        "ja": 'レーン (シングル)',
        "ko": '레인 (싱글)',
    },
    "customize_lane_double": {
        "en": 'LANE (DOUBLE)',
        "ja": 'レーン (ダブル)',
        "ko": '레인 (더블)',
    },
    "customize_lanecover_single": {
        "en": 'LANE COVER (SINGLE)',
        "ja": 'レーンカバー (シングル)',
        "ko": '레인 커버 (싱글)',
    },
    "customize_lanecover_double": {
        "en": 'LANE COVER (DOUBLE)',
        "ja": 'レーンカバー (ダブル)',
        "ko": '레인 커버 (더블)',
    },
    "customize_movie_size": {
        "en": 'VIDEO SIZE',
        "ja": 'ムービーサイズ',
        "ko": '동영상 크기',
    },
    "is_disp_weight": {
        "en": 'DISPLAY BURNED CALORIES',
        "ja": '消費カロリー表示',
        "ko": '소모 칼로리 표시',
    },
    "weight": {
        "en": 'PLAYER WEIGHT',
        "ja": '体重',
        "ko": '체중',
    },
    "overlay_scale": {
        "en": 'OVERLAY SCALE',
        "ja": 'オーバーレイサイズ',
        "ko": '오버레이 크기',
    },
    "overlay_opacity": {
        "en": 'OVERLAY OPACITY',
        "ja": 'オーバーレイ不透明度',
        "ko": '오버레이 불투명도',
    },
    "arrow_scale": {
        "en": 'ARROW SCALE',
        "ja": '矢印サイズ',
        "ko": '화살표 크기',
    },
    "arrow_opacity": {
        "en": 'ARROW OPACITY',
        "ja": '矢印不透明度',
        "ko": '화살표 불투명도',
    },
    "perspective": {
        "en": 'PERSPECTIVE',
        "ja": '視点',
        "ko": '시점',
    },
    "song_speed": {
        "en": 'SONG PLAYBACK SPEED',
        "ja": '曲の再生速度',
        "ko": '곡 재생 속도',
    },
    "preserve_pitch": {
        "en": 'PRESERVE SONG PITCH',
        "ja": '曲のピッチを保持',
        "ko": '곡 피치 유지',
    },
    "sync_movie": {
        "en": 'SYNC BACKGROUND VIDEO',
        "ja": '背景ムービーを同期',
        "ko": '배경 영상 동기화',
    },
    "training_start_time": {
        "en": 'SONG START TIME',
        "ja": '曲の開始時間',
        "ko": '곡 시작 시간',
    },
    "training_end_time": {
        "en": 'SONG END TIME',
        "ja": '曲の終了時間',
        "ko": '곡 종료 시간',
    },
    "training_loop_song": {
        "en": 'LOOP SONG',
        "ja": '曲をループ',
        "ko": '곡 반복',
    },
    "training_progress_pos": {
        "en": 'TIMELINE PLACEMENT',
        "ja": 'タイムライン表示位置',
        "ko": '타임라인 표시 위치',
    },
}

# ── Group-heading labels (seop_item_header_*) ────────────────────────────
# Keep in sync with src/mods/decorative_option_headers.rs HEADER_IDS.
HEADER_LABELS = {
    "header_power_user_options": {
        "en": 'POWER USER OPTIONS',
        "ja": 'パワーユーザー設定',
        "ko": '파워 유저 설정',
    },
    "header_playfield_styling_options": {
        "en": 'PLAYFIELD STYLING OPTIONS',
        "ja": 'プレイフィールド表示設定',
        "ko": '플레이필드 표시 설정',
    },
    "header_training_options": {
        "en": 'TRAINING OPTIONS',
        "ja": 'トレーニング設定',
        "ko": '트레이닝 설정',
    },
    "header_profile_customization_options": {
        "en": 'PROFILE CUSTOMIZATION OPTIONS',
        "ja": 'プロフィールカスタマイズ設定',
        "ko": '프로필 커스터마이즈 설정',
    },
}

# ── Value-ribbon chips (seop_op_<key>) ───────────────────────────────────
# NEVER add stock ribbon names here (on/off/left/right/...): the game ships
# those, and a generated PNG collides with the stock lookup at atlas
# injection (2026-08-15 demo finding).
RIBBONS = {
    "fullscreen": {
        "en": 'FULL SCREEN',
        "ja": 'フルスクリーン',
        "ko": '전체 화면',
    },
    "overhead": {
        "en": 'OVERHEAD',
        "ja": 'オーバーヘッド',
        "ko": '오버헤드',
    },
    "hallway": {
        "en": 'HALLWAY',
        "ja": 'ホールウェイ',
        "ko": '홀웨이',
    },
    "distant": {
        "en": 'DISTANT',
        "ja": 'ディスタント',
        "ko": '디스턴트',
    },
}

# ── Preview-box explainers (seop_image_*) ────────────────────────────────
# SPLIT panels composite their baked art (scripts/templates/<art>.png,
# extracted once from the hand-authored originals) at art_pos. The nine
# customize_* BASE chromes are DLL-generated at runtime from the _TEMPLATE
# PNGs (TEMPLATES below) — never list a base chrome here.
PREVIEWS = [
    # ── Hand-authored take-overs (art extracted 2026-08-17) ─────────────
    PreviewSpec(
        "autoplay",
        "on",
        WIDE,
        {
            "en": [
                "Every arrow reaches the STEP ZONE with a rating of "
                "MARVELOUS, regardless of player input.",
                "But, resulting scores will not be saved while this "
                "option is ON.",
            ],
            "ja": [
                'プレイヤーの入力に関係なく、すべての矢印がMARVELOUS判定でSTEP ZONEに到達します。',
                'ただし、この設定がONの間はスコアが保存されません。',
            ],
            "ko": [
                '플레이어의 입력과 관계없이 모든 화살표가 MARVELOUS 판정으로 스텝존에 도달합니다.',
                '단, 이 설정이 ON인 동안에는 스코어가 저장되지 않습니다.',
            ],
        },
    ),
    PreviewSpec(
        "autoplay",
        "off",
        WIDE,
        {
            "en": ["Standard gameplay."],
            "ja": [
                '通常のプレイです。',
            ],
            "ko": [
                '일반 플레이입니다.',
            ],
        },
    ),
    PreviewSpec(
        "premium_free",
        "on",
        SPLIT,
        {
            "en": [
                "Changes the play stage progression.",
                "ON: Play stage does not progress after every song played.",
            ],
            "ja": [
                'プレイステージの進行を変更します。',
                'ON: 曲をプレイしてもステージが進みません。',
            ],
            "ko": [
                '플레이 스테이지 진행을 변경합니다.',
                'ON: 곡을 플레이해도 스테이지가 진행되지 않습니다.',
            ],
        },
        art="premium_free_on_art.png",
        art_pos=(225, 6),
    ),
    PreviewSpec(
        "premium_free",
        "off",
        SPLIT,
        {
            "en": [
                "Changes the play stage progression.",
                "OFF: Play stage progresses after every song played, up "
                "to FINAL STAGE (EXTRA STAGE if requirements met).",
            ],
            "ja": [
                'プレイステージの進行を変更します。',
                'OFF: 曲をプレイするたびにステージが進み、FINAL STAGEまで続きます (条件を満たすとEXTRA STAGE)。',
            ],
            "ko": [
                '플레이 스테이지 진행을 변경합니다.',
                'OFF: 곡을 플레이할 때마다 스테이지가 진행되어 FINAL STAGE까지 이어집니다 (조건 충족 시 EXTRA STAGE).',
            ],
        },
        art="premium_free_off_art.png",
        art_pos=(206, 6),
    ),
    PreviewSpec(
        "center_arrows_1p",
        "on",
        SPLIT,
        {
            "en": [
                "ON: During SINGLE STYLE play, lane appears in the center "
                "of the screen.",
            ],
            "ja": [
                'ON: シングルプレイ時、レーンが画面中央に表示されます。',
            ],
            "ko": [
                'ON: 싱글 플레이 시 레인이 화면 중앙에 표시됩니다.',
            ],
        },
        art="center_arrows_1p_on_art.png",
        art_pos=(194, 40),
    ),
    PreviewSpec(
        "center_arrows_1p",
        "off",
        SPLIT,
        {
            "en": [
                "OFF: During SINGLE STYLE play, lane appears in default "
                "position.",
            ],
            "ja": [
                'OFF: シングルプレイ時、レーンが通常の位置に表示されます。',
            ],
            "ko": [
                'OFF: 싱글 플레이 시 레인이 기본 위치에 표시됩니다.',
            ],
        },
        art="center_arrows_1p_off_art.png",
        art_pos=(194, 40),
    ),
    PreviewSpec(
        "customize_movie_size",
        "on",
        SPLIT,
        {
            "en": [
                "Changes the display of song movies.",
                "ON: Song movies appear in a window during play.",
            ],
            "ja": [
                '曲のムービー表示を変更します。',
                'ON: プレイ中、ムービーがウィンドウ表示されます。',
            ],
            "ko": [
                '곡 동영상 표시를 변경합니다.',
                'ON: 플레이 중 동영상이 창 크기로 표시됩니다.',
            ],
        },
        art="customize_movie_size_on_art.png",
        art_pos=(194, 3),
    ),
    PreviewSpec(
        "customize_movie_size",
        "off",
        SPLIT,
        {
            "en": [
                "Changes the display of song movies.",
                "OFF: Song movies are not shown.",
            ],
            "ja": [
                '曲のムービー表示を変更します。',
                'OFF: ムービーは表示されません。',
            ],
            "ko": [
                '곡 동영상 표시를 변경합니다.',
                'OFF: 동영상이 표시되지 않습니다.',
            ],
        },
        art="customize_movie_size_off_art.png",
        art_pos=(194, 3),
    ),
    PreviewSpec(
        "customize_movie_size",
        "fullscreen",
        SPLIT,
        {
            "en": [
                "Changes the display of song movies.",
                "FULL SCREEN: Song movies fill entire background during "
                "play. Certain customization is not shown.",
            ],
            "ja": [
                '曲のムービー表示を変更します。',
                'FULL SCREEN: プレイ中、ムービーが背景全体に表示されます。一部のカスタマイズは表示されません。',
            ],
            "ko": [
                '곡 동영상 표시를 변경합니다.',
                'FULL SCREEN: 플레이 중 동영상이 배경 전체에 표시됩니다. 일부 커스터마이즈는 표시되지 않습니다.',
            ],
        },
        art="customize_movie_size_fullscreen_art.png",
        art_pos=(194, 3),
    ),
    PreviewSpec(
        'assist_tick',
        'off',
        WIDE,
        {
            "en": [
                'No assist sound is played during gameplay.',
            ],
            "ja": [
                'プレイ中、アシスト音は再生されません。',
            ],
            "ko": [
                '플레이 중 어시스트 사운드가 재생되지 않습니다.',
            ],
        },
    ),
    PreviewSpec(
        'assist_tick',
        'on',
        WIDE,
        {
            "en": [
                'A clap is played at every arrow in the chart, on the beat the arrow is written at.',
                'The clap follows the chart, not your steps, so it stays in time even when you miss.',
            ],
            "ja": [
                '譜面のすべての矢印のタイミングに合わせてクラップ音を再生します。',
                'クラップは譜面に追従するため、ミスをしても正確なタイミングを保ちます。',
            ],
            "ko": [
                '채보의 모든 화살표 타이밍에 맞춰 클랩 사운드를 재생합니다.',
                '클랩은 채보를 따라가므로 스텝을 놓쳐도 정확한 박자를 유지합니다.',
            ],
        },
    ),
    PreviewSpec(
        'assist_tick_volume',
        None,
        WIDE,
        {
            "en": [
                'Adjusts the volume of the clap sound played by the assist tick during gameplay.',
                'Less than 100% makes the clap quieter. Greater than 100% makes it louder.',
            ],
            "ja": [
                'アシストティックのクラップ音量を調整します。',
                '100%未満で小さく、100%超で大きくなります。',
            ],
            "ko": [
                '어시스트 틱의 클랩 음량을 조정합니다.',
                '100% 미만이면 작아지고, 100%를 넘으면 커집니다.',
            ],
        },
    ),
    PreviewSpec(
        'announcer_mute',
        'off',
        WIDE,
        {
            "en": [
                'Mutes the announcer voice during gameplay.',
                'OFF: Combo callouts, accolades and cheers play as usual.',
            ],
            "ja": [
                'プレイ中のアナウンス音声をミュートします。',
                'OFF: コンボの掛け声や歓声は通常どおり再生されます。',
            ],
            "ko": [
                '플레이 중 아나운서 음성을 음소거합니다.',
                'OFF: 콤보 콜과 환호성이 평소대로 재생됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'announcer_mute',
        'on',
        WIDE,
        {
            "en": [
                'Mutes the announcer voice during gameplay.',
                "ON: Combo callouts, accolades and cheers are silenced. In versus play, P1's choice wins.",
            ],
            "ja": [
                'プレイ中のアナウンス音声をミュートします。',
                'ON: コンボの掛け声や歓声が消音されます。対戦プレイではP1の設定が優先されます。',
            ],
            "ko": [
                '플레이 중 아나운서 음성을 음소거합니다.',
                'ON: 콤보 콜과 환호성이 음소거됩니다. 대전 플레이에서는 P1의 설정이 우선됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'skip_results_fast_exit',
        'off',
        WIDE,
        {
            "en": [
                'Chooses what happens after failing out with 3 during play.',
                'OFF: The results screen shows your score up to the exit. In versus play, the player who pressed 3 decides.',
            ],
            "ja": [
                'プレイ中に3を押して退出した後の動作を変更します。',
                'OFF: 退出時点までのスコアをリザルト画面に表示します。対戦プレイでは3を押した側の設定が適用されます。',
            ],
            "ko": [
                '플레이 중 3을 눌러 퇴장한 후의 동작을 변경합니다.',
                'OFF: 퇴장 시점까지의 스코어를 결과 화면에 표시합니다. 대전 플레이에서는 3을 누른 쪽의 설정이 적용됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'skip_results_fast_exit',
        'on',
        WIDE,
        {
            "en": [
                'Chooses what happens after failing out with 3 during play.',
                'ON: Skips the results screen and returns straight to music selection.',
            ],
            "ja": [
                'プレイ中に3を押して退出した後の動作を変更します。',
                'ON: リザルト画面をスキップして選曲画面へ直接戻ります。',
            ],
            "ko": [
                '플레이 중 3을 눌러 퇴장한 후의 동작을 변경합니다.',
                'ON: 결과 화면을 건너뛰고 곡 선택 화면으로 바로 돌아갑니다.',
            ],
        },
    ),
    PreviewSpec(
        'timing_stats',
        'off',
        WIDE,
        {
            "en": [
                'Changes the display of realtime timing statistics.',
                'OFF: No statistics are shown during play.',
            ],
            "ja": [
                'リアルタイム統計の表示を変更します。',
                'OFF: プレイ中に統計は表示されません。',
            ],
            "ko": [
                '실시간 통계 표시를 변경합니다.',
                'OFF: 플레이 중 통계가 표시되지 않습니다.',
            ],
        },
    ),
    PreviewSpec(
        'timing_stats',
        'on',
        WIDE,
        {
            "en": [
                'Changes the display of realtime timing statistics.',
                'ON: Timing error, EX loss and calories burned are shown beside each lane.',
            ],
            "ja": [
                'リアルタイム統計の表示を変更します。',
                'ON: タイミング誤差・EXロス・消費カロリーがレーン横に表示されます。',
            ],
            "ko": [
                '실시간 통계 표시를 변경합니다.',
                'ON: 타이밍 오차, EX 손실, 소모 칼로리가 레인 옆에 표시됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'pacemaker_to_mserror',
        'off',
        WIDE,
        {
            "en": [
                'Changes what the pacemaker displays.',
                'OFF: The pacemaker shows the usual score difference.',
            ],
            "ja": [
                'ペースメーカーの表示内容を変更します。',
                'OFF: 通常どおりスコア差を表示します。',
            ],
            "ko": [
                '페이스메이커 표시 내용을 변경합니다.',
                'OFF: 평소대로 스코어 차이를 표시합니다.',
            ],
        },
    ),
    PreviewSpec(
        'pacemaker_to_mserror',
        'on',
        WIDE,
        {
            "en": [
                'Changes what the pacemaker displays.',
                'ON: The pacemaker shows how early or late your last step was, in milliseconds.',
            ],
            "ja": [
                'ペースメーカーの表示内容を変更します。',
                'ON: 直前のステップの早い/遅いをミリ秒で表示します。',
            ],
            "ko": [
                '페이스메이커 표시 내용을 변경합니다.',
                'ON: 마지막 스텝이 얼마나 빠르거나 늦었는지 밀리초로 표시합니다.',
            ],
        },
    ),
    PreviewSpec(
        'pacemaker_threshold',
        None,
        WIDE,
        {
            "en": [
                'Sets how close to perfect a step must be for the pacemaker reading to turn white.',
                'Larger values widen the white zone.',
            ],
            "ja": [
                'ペースメーカーの表示が白くなる判定精度を設定します。',
                '値を大きくすると白表示の範囲が広がります。',
            ],
            "ko": [
                '페이스메이커 표시가 흰색이 되는 판정 정밀도를 설정합니다.',
                '값이 클수록 흰색 표시 범위가 넓어집니다.',
            ],
        },
    ),
    PreviewSpec(
        'step_data_export',
        'off',
        WIDE,
        {
            "en": [
                'Changes whether the timing of every step is saved to disk after each song.',
                'OFF: No file is written.',
            ],
            "ja": [
                '各ステップのタイミングを曲ごとに保存するかを変更します。',
                'OFF: ファイルは作成されません。',
            ],
            "ko": [
                '매 스텝의 타이밍을 곡마다 저장할지 변경합니다.',
                'OFF: 파일이 생성되지 않습니다.',
            ],
        },
    ),
    PreviewSpec(
        'step_data_export',
        'on',
        WIDE,
        {
            "en": [
                'Changes whether the timing of every step is saved to disk after each song.',
                'ON: A CSV file is written to the step_data_exports folder at the end of each song.',
            ],
            "ja": [
                '各ステップのタイミングを曲ごとに保存するかを変更します。',
                'ON: 曲の終了時にCSVファイルがstep_data_exportsフォルダに保存されます。',
            ],
            "ko": [
                '매 스텝의 타이밍을 곡마다 저장할지 변경합니다.',
                'ON: 곡이 끝나면 CSV 파일이 step_data_exports 폴더에 저장됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'is_disp_weight',
        'off',
        WIDE,
        {
            "en": [
                'Changes the display of calories burned.',
                'OFF: Calories are not shown.',
            ],
            "ja": [
                '消費カロリーの表示を変更します。',
                'OFF: カロリーは表示されません。',
            ],
            "ko": [
                '소모 칼로리 표시를 변경합니다.',
                'OFF: 칼로리가 표시되지 않습니다.',
            ],
        },
    ),
    PreviewSpec(
        'is_disp_weight',
        'on',
        WIDE,
        {
            "en": [
                'Changes the display of calories burned.',
                'ON: Calories are shown, estimated from the weight set below.',
            ],
            "ja": [
                '消費カロリーの表示を変更します。',
                'ON: 下で設定した体重をもとに推定したカロリーを表示します。',
            ],
            "ko": [
                '소모 칼로리 표시를 변경합니다.',
                'ON: 아래에서 설정한 체중을 바탕으로 추정한 칼로리를 표시합니다.',
            ],
        },
    ),
    PreviewSpec(
        'weight',
        None,
        WIDE,
        {
            "en": [
                'Sets your body weight, in kilograms.',
                'Used to estimate the calories burned while you play. This is saved to your profile.',
            ],
            "ja": [
                '体重をkg単位で設定します。',
                'プレイ中の消費カロリーの推定に使用され、プロフィールに保存されます。',
            ],
            "ko": [
                '체중을 kg 단위로 설정합니다.',
                '플레이 중 소모 칼로리 추정에 사용되며 프로필에 저장됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'overlay_scale',
        None,
        WIDE,
        {
            "en": [
                'Changes the size of the combo counter, judgement text and pacemaker.',
                'Applies from the next song.',
            ],
            "ja": [
                'コンボ数・判定表示・ペースメーカーのサイズを変更します。',
                '次の曲から適用されます。',
            ],
            "ko": [
                '콤보 카운터, 판정 표시, 페이스메이커의 크기를 변경합니다.',
                '다음 곡부터 적용됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'overlay_opacity',
        None,
        WIDE,
        {
            "en": [
                'Changes how solid the combo counter, judgement text and pacemaker appear.',
                '0% hides them entirely.',
            ],
            "ja": [
                'コンボ数・判定表示・ペースメーカーの不透明度を変更します。',
                '0%で完全に非表示になります。',
            ],
            "ko": [
                '콤보 카운터, 판정 표시, 페이스메이커의 불투명도를 변경합니다.',
                '0%로 설정하면 완전히 숨겨집니다.',
            ],
        },
    ),
    PreviewSpec(
        'arrow_scale',
        None,
        WIDE,
        {
            "en": [
                'Changes the size of the arrows, STEP ZONE and guidelines.',
                'The lane shrinks around the STEP ZONE. Timing and scoring are not affected.',
            ],
            "ja": [
                '矢印・STEP ZONE・ガイドラインのサイズを変更します。',
                'レーンはSTEP ZONEを中心に縮小されます。判定やスコアには影響しません。',
            ],
            "ko": [
                '화살표, 스텝존, 가이드라인의 크기를 변경합니다.',
                '레인은 스텝존을 중심으로 축소됩니다. 판정과 스코어에는 영향이 없습니다.',
            ],
        },
    ),
    PreviewSpec(
        'arrow_opacity',
        None,
        WIDE,
        {
            "en": [
                'Changes how solid the arrows, STEP ZONE and guidelines appear.',
                '0% hides them entirely. Timing and scoring are not affected.',
            ],
            "ja": [
                '矢印・STEP ZONE・ガイドラインの不透明度を変更します。',
                '0%で完全に非表示になります。判定やスコアには影響しません。',
            ],
            "ko": [
                '화살표, 스텝존, 가이드라인의 불투명도를 변경합니다.',
                '0%로 설정하면 완전히 숨겨집니다. 판정과 스코어에는 영향이 없습니다.',
            ],
        },
    ),
    PreviewSpec(
        'perspective',
        'overhead',
        WIDE,
        {
            "en": [
                'Arrows scroll at a constant size, in the standard flat view.',
            ],
            "ja": [
                '矢印は一定のサイズのまま、標準の平面ビューでスクロールします。',
            ],
            "ko": [
                '화살표가 일정한 크기로 표준 평면 뷰에서 스크롤됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'perspective',
        'hallway',
        WIDE,
        {
            "en": [
                'Arrows scroll in from a vanishing point, growing as they approach the STEP ZONE.',
            ],
            "ja": [
                '矢印は消失点から現れ、STEP ZONEに近づくにつれて大きくなります。',
            ],
            "ko": [
                '화살표가 소실점에서 나타나 스텝존에 가까워질수록 커집니다.',
            ],
        },
    ),
    PreviewSpec(
        'perspective',
        'distant',
        WIDE,
        {
            "en": [
                'Arrows start large and shrink as they approach the STEP ZONE, which sits toward the horizon.',
            ],
            "ja": [
                '矢印は大きく現れ、奥にあるSTEP ZONEに近づくにつれて小さくなります。',
            ],
            "ko": [
                '화살표가 크게 나타나 안쪽에 있는 스텝존에 가까워질수록 작아집니다.',
            ],
        },
    ),
    PreviewSpec(
        'song_speed',
        None,
        WIDE,
        {
            "en": [
                'Adjusts the rate at which the song will be played during gameplay.',
                'Less than 100% slows the song down. Greater than 100% speeds the song up.',
            ],
            "ja": [
                'プレイ中の曲の再生速度を調整します。',
                '100%未満で遅く、100%超で速くなります。',
            ],
            "ko": [
                '플레이 중 곡의 재생 속도를 조정합니다.',
                '100% 미만이면 느려지고, 100%를 넘으면 빨라집니다.',
            ],
        },
    ),
    PreviewSpec(
        'preserve_pitch',
        'off',
        WIDE,
        {
            "en": [
                "Decides whether the song's pitch should be preserved when the playback speed is adjusted.",
                "OFF: The song's pitch falls or rises with the playback speed, like a record player.",
            ],
            "ja": [
                '再生速度を変更したときに曲のピッチを保持するかを設定します。',
                'OFF: レコードのように、再生速度に合わせてピッチが上下します。',
            ],
            "ko": [
                '재생 속도를 변경할 때 곡의 피치를 유지할지 설정합니다.',
                'OFF: 레코드판처럼 재생 속도에 따라 피치가 오르내립니다.',
            ],
        },
    ),
    PreviewSpec(
        'preserve_pitch',
        'on',
        WIDE,
        {
            "en": [
                "Decides whether the song's pitch should be preserved when the playback speed is adjusted.",
                'ON: The song keeps its original pitch at any playback speed.',
            ],
            "ja": [
                '再生速度を変更したときに曲のピッチを保持するかを設定します。',
                'ON: どの再生速度でも元のピッチを保ちます。',
            ],
            "ko": [
                '재생 속도를 변경할 때 곡의 피치를 유지할지 설정합니다.',
                'ON: 어떤 재생 속도에서도 원래 피치를 유지합니다.',
            ],
        },
    ),
    PreviewSpec(
        'sync_movie',
        'off',
        WIDE,
        {
            "en": [
                'Decides whether the background video plays when the song playback speed is adjusted.',
                'OFF: Songs played at an adjusted speed show a static background instead of their video.',
            ],
            "ja": [
                '再生速度を変更したときに背景ムービーを再生するかを設定します。',
                'OFF: 速度変更時はムービーの代わりに静止背景を表示します。',
            ],
            "ko": [
                '재생 속도를 변경할 때 배경 영상을 재생할지 설정합니다.',
                'OFF: 속도가 변경된 곡은 영상 대신 고정 배경을 표시합니다.',
            ],
        },
    ),
    PreviewSpec(
        'sync_movie',
        'on',
        WIDE,
        {
            "en": [
                'Decides whether the background video plays when the song playback speed is adjusted.',
                'ON: The video plays at the adjusted speed, in sync with the song. Requires Windows.',
            ],
            "ja": [
                '再生速度を変更したときに背景ムービーを再生するかを設定します。',
                'ON: ムービーも変更後の速度で曲と同期して再生されます。Windowsが必要です。',
            ],
            "ko": [
                '재생 속도를 변경할 때 배경 영상을 재생할지 설정합니다.',
                'ON: 영상도 변경된 속도로 곡과 동기화되어 재생됩니다. Windows가 필요합니다.',
            ],
        },
    ),
    PreviewSpec(
        'training_start_time',
        None,
        WIDE,
        {
            "en": [
                'Starts the song at the chosen timestamp, in seconds. The song opens with the notes scrolling in naturally.',
                'Practice a later section without playing through the beginning. Resets when you card in.',
            ],
            "ja": [
                '指定した時間 (秒) から曲を開始します。矢印は自然にスクロールインします。',
                '序盤をプレイせずに後半を練習できます。カードイン時にリセットされます。',
            ],
            "ko": [
                '지정한 시간 (초) 부터 곡을 시작합니다. 화살표는 자연스럽게 스크롤되어 들어옵니다.',
                '앞부분을 플레이하지 않고 뒷부분을 연습할 수 있습니다. 카드 인 시 초기화됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'training_end_time',
        None,
        WIDE,
        {
            "en": [
                "Ends the song at the chosen timestamp, in seconds. Values at or past the song's length play it to the end.",
                'Practice a section without playing through the ending. Resets when you card in.',
            ],
            "ja": [
                '指定した時間 (秒) で曲を終了します。曲の長さ以上の値では最後までプレイします。',
                '終盤をプレイせずに途中の区間を練習できます。カードイン時にリセットされます。',
            ],
            "ko": [
                '지정한 시간 (초) 에서 곡을 종료합니다. 곡 길이 이상의 값이면 끝까지 플레이합니다.',
                '뒷부분을 플레이하지 않고 구간을 연습할 수 있습니다. 카드 인 시 초기화됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'training_loop_song',
        'off',
        WIDE,
        {
            "en": [
                'Decides whether the chosen song section repeats until you stop playing.',
                'OFF: Reaching the section end finishes the song normally, with results for the part you played.',
            ],
            "ja": [
                '選択した区間を繰り返すかを設定します。',
                'OFF: 区間の終わりに達すると通常どおり曲が終了し、プレイした部分のリザルトが表示されます。',
            ],
            "ko": [
                '선택한 구간을 반복할지 설정합니다.',
                'OFF: 구간 끝에 도달하면 곡이 정상적으로 종료되고, 플레이한 부분의 결과가 표시됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'training_loop_song',
        'on',
        WIDE,
        {
            "en": [
                'Decides whether the chosen song section repeats until you stop playing.',
                'ON: The section restarts each time its end is reached. Press 3 three times to give up, or 1 three times to restart.',
            ],
            "ja": [
                '選択した区間を繰り返すかを設定します。',
                'ON: 区間の終わりに達するたびに再スタートします。3を3回押すと終了、1を3回押すとリスタートします。',
            ],
            "ko": [
                '선택한 구간을 반복할지 설정합니다.',
                'ON: 구간 끝에 도달할 때마다 다시 시작됩니다. 3을 세 번 누르면 포기, 1을 세 번 누르면 재시작합니다.',
            ],
        },
    ),
    PreviewSpec(
        'training_progress_pos',
        None,
        WIDE,
        {
            "en": [
                'Shows the song timeline during play on the chosen edge of the screen. OFF hides it.',
                'The timeline draws the whole chart with your position, section markers, and per-measure lines. Follows your card.',
            ],
            "ja": [
                'プレイ中、画面の選択した端にタイムラインを表示します。OFFで非表示になります。',
                'タイムラインには譜面全体・現在位置・区間マーカー・小節線が表示されます。カードに保存されます。',
            ],
            "ko": [
                '플레이 중 화면의 선택한 가장자리에 타임라인을 표시합니다. OFF면 숨겨집니다.',
                '타임라인에는 전체 채보, 현재 위치, 구간 마커, 마디선이 표시됩니다. 카드에 저장됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'adjust_song_offset',
        'off',
        WIDE,
        {
            "en": [
                'No per-song offset is stored for the highlighted song.',
                'Your normal JUDGEMENT OFFSET setting applies when you play it.',
            ],
            "ja": [
                '選択中の曲には個別オフセットが保存されていません。',
                'プレイ時は通常の判定タイミング設定が適用されます。',
            ],
            "ko": [
                '선택한 곡에 개별 오프셋이 저장되어 있지 않습니다.',
                '플레이 시 일반 판정 타이밍 설정이 적용됩니다.',
            ],
        },
    ),
    PreviewSpec(
        'adjust_song_offset',
        'on',
        WIDE,
        {
            "en": [
                'Stores a judgement offset for the highlighted song only, overriding your JUDGEMENT OFFSET while it plays.',
                'The value is saved per song and follows the song wheel. An offset of 0 is a valid override.',
            ],
            "ja": [
                '選択中の曲にのみ適用される判定オフセットを保存し、プレイ中は判定タイミング設定を上書きします。',
                '値は曲ごとに保存され、選曲に追従します。0も有効なオフセットです。',
            ],
            "ko": [
                '선택한 곡에만 적용되는 판정 오프셋을 저장하여 플레이 중 판정 타이밍 설정을 덮어씁니다.',
                '값은 곡별로 저장되며 선곡을 따라갑니다. 0도 유효한 오프셋입니다.',
            ],
        },
    ),
    PreviewSpec(
        'current_song_offset',
        None,
        WIDE,
        {
            "en": [
                'The judgement offset used for the highlighted song, in milliseconds (-100 to +100).',
                'Positive values judge later, negative values judge earlier — same scale as JUDGEMENT OFFSET.',
            ],
            "ja": [
                '選択中の曲に適用される判定オフセットです（-100〜+100ミリ秒）。',
                'プラスで判定が遅く、マイナスで早くなります。判定タイミングと同じ尺度です。',
            ],
            "ko": [
                '선택한 곡에 적용되는 판정 오프셋입니다 (-100 ~ +100 밀리초).',
                '양수면 판정이 늦어지고 음수면 빨라집니다. 판정 타이밍과 같은 척도입니다.',
            ],
        },
    ),
]

# ── Customize preview-chrome templates ───────────────────────────────────
# Marker geometry measured off the shipped hand-authored templates
# (2026-08-17); byte-identical across languages by construction. NOTE:
# customize_character_p2's shipped English said "left side" (copy/paste of
# p1) — corrected to "right" here in all languages.
TEMPLATES = {
    "customize_appeal_board": TemplateSpec(
        "customize_appeal_board",
        [(191, 33, 170, 22, (255, 0, 0, 255)), (191, 67, 170, 70, (0, 255, 0, 255))],
        {
            "en": ["Sets the appeal board", "appearance."],
            "ja": [
                'アピールボードの',
                '外観を設定します。',
            ],
            "ko": [
                '어필 보드의 모양을',
                '설정합니다.',
            ],
        },
    ),
    "customize_background": TemplateSpec(
        "customize_background",
        [(196, 41, 160, 90, (0, 255, 0, 255))],
        {
            "en": ["Changes the", "background of menus."],
            "ja": [
                'メニュー画面の',
                '背景を変更します。',
            ],
            "ko": [
                '메뉴 화면의 배경을',
                '변경합니다.',
            ],
        },
    ),
    "customize_background_gameplay": TemplateSpec(
        "customize_background_gameplay",
        [(196, 41, 160, 90, (0, 255, 0, 255))],
        {
            "en": ["Changes the", "background during", "gameplay."],
            "ja": [
                'プレイ中の背景を',
                '変更します。',
            ],
            "ko": [
                '플레이 중 배경을',
                '변경합니다.',
            ],
        },
    ),
    "customize_character_p1": TemplateSpec(
        "customize_character_p1",
        [(209, 11, 134, 150, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the character",
                "illustration on the left",
                "side of the screen.",
            ],
            "ja": [
                '画面左側のキャラクター',
                'イラストを変更します。',
            ],
            "ko": [
                '화면 왼쪽의 캐릭터',
                '일러스트를 변경합니다.',
            ],
        },
    ),
    "customize_character_p2": TemplateSpec(
        "customize_character_p2",
        [(209, 11, 134, 150, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the character",
                "illustration on the right",
                "side of the screen.",
            ],
            "ja": [
                '画面右側のキャラクター',
                'イラストを変更します。',
            ],
            "ko": [
                '화면 오른쪽의 캐릭터',
                '일러스트를 변경합니다.',
            ],
        },
    ),
    "customize_lane_single": TemplateSpec(
        "customize_lane_single",
        [(234, 13, 88, 147, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the",
                "appearance of the lane,",
                "for SINGLE STYLE.",
            ],
            "ja": [
                'レーンの外観を',
                '変更します',
                '(シングルプレイ用)。',
            ],
            "ko": [
                '레인의 모양을',
                '변경합니다',
                '(싱글 플레이용).',
            ],
        },
    ),
    "customize_lane_double": TemplateSpec(
        "customize_lane_double",
        [(196, 18, 162, 137, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the",
                "appearance of the lane,",
                "for DOUBLE STYLE.",
            ],
            "ja": [
                'レーンの外観を',
                '変更します',
                '(ダブルプレイ用)。',
            ],
            "ko": [
                '레인의 모양을',
                '변경합니다',
                '(더블 플레이용).',
            ],
        },
    ),
    "customize_lanecover_single": TemplateSpec(
        "customize_lanecover_single",
        [(233, 11, 93, 151, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the",
                "illustration used as the",
                "lane cover during",
                "gameplay, for SINGLE",
                "STYLE.",
            ],
            "ja": [
                'プレイ中のレーン',
                'カバーのイラストを',
                '変更します',
                '(シングルプレイ用)。',
            ],
            "ko": [
                '플레이 중 레인 커버',
                '일러스트를 변경합니다',
                '(싱글 플레이용).',
            ],
        },
    ),
    "customize_lanecover_double": TemplateSpec(
        "customize_lanecover_double",
        [(195, 20, 162, 133, (0, 255, 0, 255))],
        {
            "en": [
                "Changes the",
                "illustration used as the",
                "lane cover during",
                "gameplay, for DOUBLE",
                "STYLE.",
            ],
            "ja": [
                'プレイ中のレーン',
                'カバーのイラストを',
                '変更します',
                '(ダブルプレイ用)。',
            ],
            "ko": [
                '플레이 중 레인 커버',
                '일러스트를 변경합니다',
                '(더블 플레이용).',
            ],
        },
    ),
}

# ── Verbatim per-language copies ─────────────────────────────────────────
# Textures with no translatable text, shipped identically in every language
# dir: the return-button floppy icon and the Latin "Modpack" brand wordmark.
# texture name -> scripts/templates/ master file.
VERBATIM_COPIES = {
    "seop_return": "seop_return_master.png",
    "seop_tab_title_mods": "seop_tab_title_mods_master.png",
}
