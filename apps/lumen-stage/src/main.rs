//! Independent 9:16 broadcast scene. A disconnected runtime never freezes the render loop.
use bevy::{
    prelude::*,
    text::{LineBreak, TextBounds},
};
use lumen_protocol::{Command, Response, StageState};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Resource)]
struct Link {
    receiver: Mutex<mpsc::Receiver<Result<Response, String>>>,
    running: Arc<AtomicBool>,
    state: StageState,
    status: String,
}
impl Drop for Link {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
#[derive(Component)]
enum Label {
    Status,
    Topic,
    Caption,
    Question,
    Name(usize),
}
#[derive(Component)]
struct Mouth(usize);
#[derive(Component)]
struct Halo(usize);

fn main() {
    let socket = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap_or_else(|| "/tmp".into()))
                .join("lumen/control.sock")
        });
    let (sender, receiver) = mpsc::sync_channel(1);
    let running = Arc::new(AtomicBool::new(true));
    let worker = running.clone();
    std::thread::spawn(move || {
        while worker.load(Ordering::Relaxed) {
            let response =
                lumen_protocol::request(&socket, Command::Status, Duration::from_millis(500))
                    .map_err(|e| e.to_string());
            let _ = sender.try_send(response);
            std::thread::sleep(Duration::from_millis(100));
        }
    });
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.035, 0.055, 0.09)))
        .insert_resource(Link {
            receiver: Mutex::new(receiver),
            running,
            state: StageState::default(),
            status: "CONNECTING".into(),
        })
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Lumen Stage — synthetic hosts".into(),
                resolution: (1080, 1920).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, update)
        .run();
}

fn rect(commands: &mut Commands, color: Color, size: Vec2, position: Vec3) {
    commands.spawn((
        Sprite::from_color(color, size),
        Transform::from_translation(position),
    ));
}
fn circle(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ColorMaterial>,
    color: Color,
    radius: f32,
    position: Vec3,
) {
    commands.spawn((
        Mesh2d(meshes.add(Circle::new(radius))),
        MeshMaterial2d(materials.add(color)),
        Transform::from_translation(position),
    ));
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    commands.spawn((
        Camera2d,
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed {
                width: 1080.,
                height: 1920.,
            },
            ..OrthographicProjection::default_2d()
        }),
    ));
    rect(
        &mut commands,
        Color::srgb(0.09, 0.14, 0.21),
        Vec2::new(880., 200.),
        Vec3::new(0., 580., 0.),
    );
    rect(
        &mut commands,
        Color::srgb(0.07, 0.11, 0.18),
        Vec2::new(880., 290.),
        Vec3::new(0., -530., 0.),
    );
    commands.spawn((
        Text2d::new("LUMEN"),
        TextFont {
            font_size: FontSize::Px(76.),
            ..default()
        },
        TextColor(Color::srgb(0.87, 0.94, 0.98)),
        Transform::from_xyz(0., 850., 10.),
    ));
    commands.spawn((
        Text2d::new("SYNTHETIC HOSTS • REAL ARGUMENTS"),
        TextFont {
            font_size: FontSize::Px(22.),
            ..default()
        },
        TextColor(Color::srgb(0.49, 0.65, 0.76)),
        Transform::from_xyz(0., 790., 10.),
    ));
    let specs = [
        (Label::Status, 0., 720., 24., 840., 50.),
        (Label::Topic, 0., 580., 36., 820., 180.),
        (Label::Caption, 0., -530., 38., 820., 260.),
        (Label::Question, 0., -760., 26., 820., 100.),
        (Label::Name(0), -245., -280., 38., 420., 100.),
        (Label::Name(1), 245., -280., 38., 420., 100.),
    ];
    for (label, x, y, font, width, height) in specs {
        commands.spawn((
            Text2d::new(""),
            TextFont {
                font_size: FontSize::Px(font),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.94, 0.99)),
            TextLayout::new(Justify::Center, LineBreak::WordBoundary),
            TextBounds::from(Vec2::new(width, height)),
            Transform::from_xyz(x, y, 10.),
            label,
        ));
    }
    for (index, x, accent, skin) in [
        (
            0,
            -245.,
            Color::srgb(0.18, 0.7, 0.68),
            Color::srgb(0.78, 0.54, 0.39),
        ),
        (
            1,
            245.,
            Color::srgb(0.93, 0.49, 0.3),
            Color::srgb(0.91, 0.72, 0.56),
        ),
    ] {
        commands.spawn((
            Sprite::from_color(accent.with_alpha(0.08), Vec2::new(340., 560.)),
            Transform::from_xyz(x, 45., 0.),
            Halo(index),
        ));
        rect(
            &mut commands,
            accent,
            Vec2::new(235., 340.),
            Vec3::new(x, -30., 1.),
        );
        rect(
            &mut commands,
            Color::srgb(0.05, 0.12, 0.17),
            Vec2::new(42., 340.),
            Vec3::new(x, -30., 2.),
        );
        circle(
            &mut commands,
            &mut meshes,
            &mut materials,
            Color::srgb(0.03, 0.05, 0.08),
            110.,
            Vec3::new(x, 180., 3.),
        );
        circle(
            &mut commands,
            &mut meshes,
            &mut materials,
            skin,
            100.,
            Vec3::new(x, 180., 4.),
        );
        rect(
            &mut commands,
            Color::srgb(0.05, 0.07, 0.11),
            Vec2::new(180., 45.),
            Vec3::new(x, 259., 5.),
        );
        for eye in [-30., 30.] {
            circle(
                &mut commands,
                &mut meshes,
                &mut materials,
                Color::srgb(0.96, 0.96, 0.92),
                12.,
                Vec3::new(x + eye, 205., 5.),
            );
            circle(
                &mut commands,
                &mut meshes,
                &mut materials,
                Color::srgb(0.04, 0.07, 0.1),
                6.,
                Vec3::new(x + eye, 205., 6.),
            );
            rect(
                &mut commands,
                Color::srgb(0.1, 0.08, 0.08),
                Vec2::new(36., 6.),
                Vec3::new(x + eye, 231., 6.),
            );
        }
        if index == 1 {
            rect(
                &mut commands,
                Color::srgb(0.12, 0.08, 0.06),
                Vec2::new(110., 6.),
                Vec3::new(x, 205., 7.),
            );
        }
        commands.spawn((
            Sprite::from_color(Color::srgb(0.22, 0.07, 0.09), Vec2::new(44., 7.)),
            Transform::from_xyz(x, 143., 6.),
            Mouth(index),
        ));
    }
}

fn update(
    mut link: ResMut<Link>,
    mut labels: Query<(&mut Text2d, &Label)>,
    mut mouths: Query<(&mut Transform, &Mouth)>,
    mut halos: Query<(&mut Sprite, &Halo)>,
) {
    let messages: Vec<_> = link
        .receiver
        .lock()
        .map(|receiver| receiver.try_iter().collect())
        .unwrap_or_default();
    for message in messages {
        match message {
            Ok(response) => {
                link.status = if response.stopped {
                    "EMERGENCY STOP"
                } else if response.paused {
                    "PAUSED"
                } else if response.muted {
                    "MUTED"
                } else if response.stage.agents[0].is_empty() {
                    "WAITING FOR SHOW"
                } else {
                    "DEBATE"
                }
                .into();
                link.state = response.stage;
            }
            Err(_) => {
                link.status = "RUNTIME DISCONNECTED".into();
                link.state.clear_speech();
            }
        }
    }
    for (mut text, label) in &mut labels {
        let value = match label {
            Label::Status => link.status.clone(),
            Label::Topic => {
                if link.state.topic.is_empty() {
                    "A new conversation is waiting.".into()
                } else {
                    link.state.topic.chars().take(180).collect()
                }
            }
            Label::Caption => link.state.caption.clone(),
            Label::Question => link
                .state
                .question
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(150)
                .collect(),
            Label::Name(index) => link.state.agents[*index].chars().take(32).collect(),
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for (mut transform, mouth) in &mut mouths {
        let speaking = link.state.speaker.as_ref() == Some(&link.state.agents[mouth.0]);
        transform.scale.y = if speaking {
            1. + link.state.level as f32 * 0.12
        } else {
            1.
        };
    }
    for (mut sprite, halo) in &mut halos {
        let speaking = link.state.speaker.as_ref() == Some(&link.state.agents[halo.0]);
        sprite.color = sprite.color.with_alpha(if speaking { 0.24 } else { 0.08 });
    }
}
