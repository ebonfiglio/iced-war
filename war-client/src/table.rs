use iced::advanced::graphics::text::Paragraph;
use iced::advanced::text::{self, Paragraph as _, Shaping, Wrapping};
use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::canvas::{self, Action, Event, Frame, Geometry, Program};
use iced::widget::image::Handle;
use iced::{Color, Pixels, Point, Rectangle, Renderer, Size, Theme, Vector};
use war_protocol::{Card, PlayerGameView, Rank, RoundResult, Suit};

use crate::Message;

const TABLE_SIZE: Size = Size::new(1536.0, 1024.0);
const CARD_SIZE: Size = Size::new(126.0, 171.0);
const SEAT_X: f32 = 766.0;
const DECK_X: f32 = SEAT_X - 100.0;
const BATTLE_X: f32 = SEAT_X + 100.0;
// The name anchors are the centers of the circles in poketable.png.
const OPPONENT_SEAT: Seat = Seat {
    name_y: 169.0,
    card_y: 347.0,
    count_y: 455.0,
};
const PLAYER_SEAT: Seat = Seat {
    name_y: 832.0,
    card_y: 656.0,
    count_y: 548.0,
};
const DECK_OFFSET: Vector = Vector::new(5.0, 4.0);
const INK: Color = Color::from_rgb(0.96, 0.93, 0.86);
const GOLD: Color = Color::from_rgb(0.94, 0.76, 0.40);

macro_rules! faces {
    ($suit:literal) => {
        faces!($suit; "2", "3", "4", "5", "6", "7", "8", "9", "10", "J", "Q", "K", "A")
    };
    ($suit:literal; $($rank:literal),+) => {
        [$(Handle::from_bytes(&include_bytes!(concat!("../assets/Cards/card", $suit, $rank, ".png"))[..])),+]
    };
}

pub(super) struct TableArt {
    table: Handle,
    opponent_back: Handle,
    player_back: Handle,
    clubs: [Handle; Rank::ALL.len()],
    diamonds: [Handle; Rank::ALL.len()],
    hearts: [Handle; Rank::ALL.len()],
    spades: [Handle; Rank::ALL.len()],
}

impl Default for TableArt {
    fn default() -> Self {
        Self {
            table: Handle::from_bytes(&include_bytes!("../assets/poketable.png")[..]),
            opponent_back: Handle::from_bytes(
                &include_bytes!("../assets/Cards/cardBack_red2.png")[..],
            ),
            player_back: Handle::from_bytes(
                &include_bytes!("../assets/Cards/cardBack_blue2.png")[..],
            ),
            clubs: faces!("Clubs"),
            diamonds: faces!("Diamonds"),
            hearts: faces!("Hearts"),
            spades: faces!("Spades"),
        }
    }
}

impl TableArt {
    fn face(&self, card: Card) -> &Handle {
        let suit = match card.suit {
            Suit::Clubs => &self.clubs,
            Suit::Diamonds => &self.diamonds,
            Suit::Hearts => &self.hearts,
            Suit::Spades => &self.spades,
        };
        &suit[card.rank as usize]
    }
}

pub(super) struct Table<'a> {
    pub art: &'a TableArt,
    pub game: &'a PlayerGameView,
    pub player_name: &'a str,
    pub last_round: Option<&'a RoundResult>,
    pub play_pending: bool,
    pub can_play: bool,
}

struct Seat {
    name_y: f32,
    card_y: f32,
    count_y: f32,
}

enum Slot {
    Empty,
    Hidden,
    Face(Card),
}

impl Table<'_> {
    fn battle_cards(&self) -> [Slot; 2] {
        let player_played = self.game.you_played || self.play_pending;
        if !self.game.opponent_played
            && !player_played
            && let Some(round) = self.last_round
        {
            return [Slot::Face(round.opponent_card), Slot::Face(round.your_card)];
        }

        [
            if self.game.opponent_played {
                Slot::Hidden
            } else {
                Slot::Empty
            },
            if player_played {
                Slot::Hidden
            } else {
                Slot::Empty
            },
        ]
    }

    fn over_player_deck(&self, bounds: Rectangle, cursor: mouse::Cursor) -> bool {
        if !self.can_play {
            return false;
        }

        let layout = Layout::new(bounds.size());
        let deck = layout.card(DECK_X, PLAYER_SEAT.card_y);
        let spread = deck_offset(self.game.your_cards_remaining) * layout.scale;
        let hit_area = Rectangle {
            width: deck.width + spread.x,
            height: deck.height + spread.y,
            ..deck
        };
        cursor
            .position_in(bounds)
            .is_some_and(|point| hit_area.contains(point))
    }
}

impl Program<Message> for Table<'_> {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) && self.over_player_deck(bounds, cursor)
        {
            Some(Action::publish(Message::PlayCard).and_capture())
        } else {
            None
        }
    }

    fn mouse_interaction(
        &self,
        _state: &(),
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.over_player_deck(bounds, cursor) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let layout = Layout::new(bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), Color::BLACK);
        frame.draw_image(layout.table, &self.art.table);

        let seats = [
            (
                OPPONENT_SEAT,
                self.game.opponent_cards_remaining,
                &self.art.opponent_back,
                self.game.opponent_name.as_str(),
                false,
            ),
            (
                PLAYER_SEAT,
                self.game.your_cards_remaining,
                &self.art.player_back,
                self.player_name,
                self.can_play,
            ),
        ];
        for ((seat, remaining, back, name, clickable), slot) in
            seats.into_iter().zip(self.battle_cards())
        {
            let deck = layout.card(DECK_X, seat.card_y);
            for layer in (0..remaining.min(3)).rev() {
                let offset = DECK_OFFSET * (f32::from(layer) * layout.scale);
                frame.draw_image(deck + offset, back);
            }

            let battle = layout.card(BATTLE_X, seat.card_y);
            match slot {
                Slot::Empty => layout.label(
                    &mut frame,
                    BATTLE_X,
                    seat.card_y,
                    "Not played".into(),
                    26.0,
                    INK.scale_alpha(0.60),
                ),
                Slot::Hidden => frame.draw_image(battle, back),
                Slot::Face(card) => frame.draw_image(battle, self.art.face(card)),
            }

            let (deck_label, color) = if clickable {
                (format!("{remaining} left / Click to play"), GOLD)
            } else {
                (format!("{remaining} left"), INK)
            };
            layout.label(&mut frame, DECK_X, seat.count_y, deck_label, 28.0, color);
            layout.seat_name(&mut frame, &seat, name);
        }

        layout.label(
            &mut frame,
            SEAT_X,
            502.0,
            format!("Pot: {}", self.game.battle_pot),
            34.0,
            GOLD,
        );

        vec![frame.into_geometry()]
    }
}

fn deck_offset(remaining: u8) -> Vector {
    DECK_OFFSET * f32::from(remaining.min(3).saturating_sub(1))
}

struct Layout {
    table: Rectangle,
    scale: f32,
}

impl Layout {
    fn new(size: Size) -> Self {
        let scale = (size.width / TABLE_SIZE.width).min(size.height / TABLE_SIZE.height);
        let table = Rectangle {
            x: (size.width - TABLE_SIZE.width * scale) / 2.0,
            y: (size.height - TABLE_SIZE.height * scale) / 2.0,
            width: TABLE_SIZE.width * scale,
            height: TABLE_SIZE.height * scale,
        };
        Self { table, scale }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(self.table.x + x * self.scale, self.table.y + y * self.scale)
    }

    fn card(&self, x: f32, y: f32) -> Rectangle {
        let top_left = self.point(x - CARD_SIZE.width / 2.0, y - CARD_SIZE.height / 2.0);
        Rectangle {
            x: top_left.x,
            y: top_left.y,
            width: CARD_SIZE.width * self.scale,
            height: CARD_SIZE.height * self.scale,
        }
    }

    fn seat_name(&self, frame: &mut Frame, seat: &Seat, name: &str) {
        let name = name.replace(['\n', '\r', '\t'], " ");
        let font = iced::Font::DEFAULT;
        let size = 34.0;
        let width = Paragraph::with_text(text::Text {
            content: name.as_str(),
            bounds: Size::INFINITE,
            size: Pixels(size),
            line_height: text::LineHeight::default(),
            font,
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            shaping: Shaping::Advanced,
            wrapping: Wrapping::None,
        })
        .min_width();
        let fitted_size = size * (124.0 / width.max(124.0));

        self.label(frame, SEAT_X, seat.name_y, name, fitted_size, INK);
    }

    fn label(&self, frame: &mut Frame, x: f32, y: f32, content: String, size: f32, color: Color) {
        frame.fill_text(canvas::Text {
            content,
            position: self.point(x, y),
            color,
            size: Pixels(size * self.scale),
            align_x: Horizontal::Center.into(),
            align_y: Vertical::Center,
            shaping: Shaping::Advanced,
            ..canvas::Text::default()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::game;
    use iced::advanced::graphics::geometry::Renderer as _;
    use iced::advanced::renderer::Headless;
    use war_protocol::RoundOutcome;

    #[test]
    fn clicking_the_resized_player_deck_plays_only_when_enabled() {
        let art = TableArt::default();
        let game = game();
        let mut table = Table {
            art: &art,
            game: &game,
            player_name: "Host",
            last_round: None,
            play_pending: false,
            can_play: true,
        };
        let bounds = Rectangle {
            x: 20.0,
            y: 45.0,
            width: 820.0,
            height: 510.0,
        };
        let cursor_at = |x, y| {
            let point = Layout::new(bounds.size()).point(x, y);
            mouse::Cursor::Available(point + Vector::new(bounds.x, bounds.y))
        };
        let click = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let player = cursor_at(666.0, 656.0);

        let action = table.update(&mut (), &click, bounds, player).unwrap();
        assert!(matches!(action.into_inner().0, Some(Message::PlayCard)));
        assert!(
            table
                .update(&mut (), &click, bounds, cursor_at(666.0, 347.0))
                .is_none()
        );

        table.can_play = false;
        assert!(table.update(&mut (), &click, bounds, player).is_none());
    }

    #[tokio::test]
    async fn renders_the_felt_card_back_and_revealed_faces() {
        let mut renderer = Renderer::new(iced::Font::DEFAULT, Pixels(16.0), Some("tiny-skia"))
            .await
            .expect("Iced software renderer");
        let size = Size::new(768, 512);
        let bounds = Rectangle::with_size(Size::new(size.width as f32, size.height as f32));
        let render = |table: Table<'_>, renderer: &mut Renderer| {
            iced::advanced::Renderer::reset(renderer, bounds);
            for geometry in table.draw(
                &(),
                renderer,
                &Theme::Dark,
                bounds,
                mouse::Cursor::Unavailable,
            ) {
                renderer.draw_geometry(geometry);
            }
            renderer.screenshot(size, 1.0, Color::BLACK)
        };
        let pixel = |image: &[u8], x, y| {
            let offset = ((y * size.width + x) * 4) as usize;
            <[u8; 3]>::try_from(&image[offset..offset + 3]).unwrap()
        };

        let art = TableArt::default();
        let mut game = game();
        let result = RoundResult {
            your_card: Card {
                rank: Rank::Ace,
                suit: Suit::Spades,
            },
            opponent_card: Card {
                rank: Rank::Two,
                suit: Suit::Hearts,
            },
            outcome: RoundOutcome::Win,
            points_awarded_to_you: 2,
        };
        let render_state = |game: &PlayerGameView, player_name, renderer: &mut Renderer| {
            render(
                Table {
                    art: &art,
                    game,
                    player_name,
                    last_round: Some(&result),
                    play_pending: false,
                    can_play: false,
                },
                renderer,
            )
        };

        game.you_played = true;
        let waiting = render_state(&game, "Host", &mut renderer);
        let [red, green, blue] = pixel(&waiting, 480, 260);
        assert!(green > red && green > blue, "expected green felt");
        let [red, green, blue] = pixel(&waiting, 433, 328);
        assert!(
            blue > red && blue > green,
            "expected the blue back, not last round's card"
        );
        let [red, green, blue] = pixel(&waiting, 450, 188);
        assert!(
            green > red && green > blue,
            "opponent's card should not be shown yet"
        );

        game.you_played = false;
        let revealed = render_state(&game, "Host", &mut renderer);
        for y in [188, 343] {
            assert!(
                pixel(&revealed, 450, y)
                    .into_iter()
                    .all(|value| value > 230),
                "expected the white face of a revealed card",
            );
        }

        let badge_differs = |variant: &[u8], y: u32| {
            (y - 11..=y + 11)
                .any(|row| (353..=413).any(|x| pixel(&revealed, x, row) != pixel(variant, x, row)))
        };
        let renamed_player = render_state(&game, "Alice", &mut renderer);
        assert!(
            badge_differs(&renamed_player, 416),
            "changing your name should redraw the bottom circle"
        );
        assert!(
            !badge_differs(&renamed_player, 84),
            "changing your name should not redraw the top circle"
        );

        game.opponent_name = "Bob".into();
        let renamed_opponent = render_state(&game, "Host", &mut renderer);
        assert!(
            badge_differs(&renamed_opponent, 84),
            "changing the opponent's name should redraw the top circle"
        );
        assert!(!badge_differs(&renamed_opponent, 416));
    }
}
