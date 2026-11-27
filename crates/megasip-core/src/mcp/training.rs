//! Agent Training: a grid exercise for a connected agent, drawn in the window's dial pad
//! while the agent has it open. A line of cells moves forward one cell per step; the agent
//! turns it left or right relative to its heading. Eating food grows it by one cell and
//! scores a point; running into the wall or into itself ends the run.

use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::{json, Value};
use std::collections::VecDeque;

pub const DEFAULT_SIZE: i32 = 16;
pub const MIN_SIZE: i32 = 8;
pub const MAX_SIZE: i32 = 32;
const START_LEN: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heading {
    Up,
    Right,
    Down,
    Left,
}

impl Heading {
    fn left(self) -> Self {
        match self {
            Heading::Up => Heading::Left,
            Heading::Left => Heading::Down,
            Heading::Down => Heading::Right,
            Heading::Right => Heading::Up,
        }
    }

    fn right(self) -> Self {
        match self {
            Heading::Up => Heading::Right,
            Heading::Right => Heading::Down,
            Heading::Down => Heading::Left,
            Heading::Left => Heading::Up,
        }
    }

    fn delta(self) -> (i32, i32) {
        match self {
            Heading::Up => (0, -1),
            Heading::Right => (1, 0),
            Heading::Down => (0, 1),
            Heading::Left => (-1, 0),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Heading::Up => "up",
            Heading::Right => "right",
            Heading::Down => "down",
            Heading::Left => "left",
        }
    }

    fn glyph(self) -> char {
        match self {
            Heading::Up => '^',
            Heading::Right => '>',
            Heading::Down => 'v',
            Heading::Left => '<',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Left,
    Right,
    Straight,
}

impl Turn {
    pub fn parse(s: &str) -> Result<Turn, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" | "l" => Ok(Turn::Left),
            "right" | "r" => Ok(Turn::Right),
            "straight" | "forward" | "s" | "f" | "" => Ok(Turn::Straight),
            other => Err(format!(
                "\"{other}\" isn't a direction: use left, right or straight"
            )),
        }
    }
}

/// What a step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Moved,
    Ate,
    /// Hit the wall or itself.
    Crashed,
    /// Filled the whole board.
    Won,
    /// Not running.
    Idle,
}

pub struct Training {
    size: i32,
    /// Head first.
    body: VecDeque<(i32, i32)>,
    heading: Heading,
    /// Real-time mode: the turn to make on the next tick.
    pending: Option<Turn>,
    food: Option<(i32, i32)>,
    score: u32,
    best: u32,
    alive: bool,
    visible: bool,
    steps: u64,
    /// Real-time mode: milliseconds per step.
    tick_ms: Option<u64>,
    /// Bumped on every start, so a stale real-time ticker stops.
    generation: u64,
    last: String,
    rng: StdRng,
}

impl Default for Training {
    fn default() -> Self {
        Training {
            size: DEFAULT_SIZE,
            body: VecDeque::new(),
            heading: Heading::Right,
            pending: None,
            food: None,
            score: 0,
            best: 0,
            alive: false,
            visible: false,
            steps: 0,
            tick_ms: None,
            generation: 0,
            last: String::new(),
            rng: StdRng::from_entropy(),
        }
    }
}

impl Training {
    #[cfg(test)]
    fn seeded(seed: u64) -> Self {
        Training {
            rng: StdRng::seed_from_u64(seed),
            ..Default::default()
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    pub fn alive(&self) -> bool {
        self.alive
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn tick_ms(&self) -> Option<u64> {
        self.tick_ms
    }

    /// A new run on a `size` x `size` board, shown in the window. `tick_ms`: move on its
    /// own every that many milliseconds; None: one step per command.
    pub fn start(&mut self, size: i32, tick_ms: Option<u64>) {
        self.size = size.clamp(MIN_SIZE, MAX_SIZE);
        let y = self.size / 2;
        let x = self.size / 2;
        self.body = (0..START_LEN as i32).map(|i| (x - i, y)).collect();
        self.heading = Heading::Right;
        self.pending = None;
        self.score = 0;
        self.alive = true;
        self.visible = true;
        self.steps = 0;
        self.tick_ms = tick_ms;
        self.generation += 1;
        self.last = "Started".into();
        self.place_food();
    }

    pub fn stop(&mut self) {
        self.alive = false;
        self.visible = false;
        self.pending = None;
        self.generation += 1;
        self.last = "Stopped".into();
    }

    fn place_food(&mut self) {
        let free: Vec<(i32, i32)> = (0..self.size)
            .flat_map(|y| (0..self.size).map(move |x| (x, y)))
            .filter(|c| !self.body.contains(c))
            .collect();
        self.food = if free.is_empty() {
            None
        } else {
            Some(free[self.rng.gen_range(0..free.len())])
        };
    }

    fn apply_turn(&mut self, t: Turn) {
        self.heading = match t {
            Turn::Left => self.heading.left(),
            Turn::Right => self.heading.right(),
            Turn::Straight => self.heading,
        };
    }

    /// Turn-based mode: turn now. Real-time mode: turn on the next tick.
    pub fn turn(&mut self, t: Turn) {
        if self.tick_ms.is_some() {
            self.pending = Some(t);
        } else {
            self.apply_turn(t);
        }
    }

    /// Move one cell forward.
    pub fn step(&mut self) -> Step {
        if !self.alive {
            return Step::Idle;
        }
        if let Some(t) = self.pending.take() {
            self.apply_turn(t);
        }
        let (dx, dy) = self.heading.delta();
        let (hx, hy) = self.body[0];
        let next = (hx + dx, hy + dy);
        let eating = self.food == Some(next);
        let out = next.0 < 0 || next.1 < 0 || next.0 >= self.size || next.1 >= self.size;
        // The tail moves out of the way this step unless we grow.
        let body_len = if eating {
            self.body.len()
        } else {
            self.body.len() - 1
        };
        let bitten = self.body.iter().take(body_len).any(|c| *c == next);
        self.steps += 1;
        if out || bitten {
            self.alive = false;
            self.best = self.best.max(self.score);
            self.last = if out {
                "Crashed into the wall".into()
            } else {
                "Crashed into itself".into()
            };
            return Step::Crashed;
        }
        self.body.push_front(next);
        if eating {
            self.score += 1;
            self.best = self.best.max(self.score);
            self.place_food();
            if self.food.is_none() {
                self.alive = false;
                self.last = "Filled the board".into();
                return Step::Won;
            }
            self.last = "Ate".into();
            Step::Ate
        } else {
            self.body.pop_back();
            self.last = "Moved".into();
            Step::Moved
        }
    }

    /// Real-time tick from the ticker of run `generation`. False when that ticker should stop.
    pub fn tick(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.alive {
            return false;
        }
        self.step();
        self.alive
    }

    /// For the window.
    pub fn to_json(&self) -> Value {
        json!({
            "visible": self.visible,
            "size": self.size,
            "cells": self.body.iter().map(|(x, y)| [x, y]).collect::<Vec<_>>(),
            "food": self.food.map(|(x, y)| [x, y]),
            "heading": self.heading.name(),
            "score": self.score,
            "best": self.best,
            "alive": self.alive,
            "steps": self.steps,
        })
    }

    /// For the agent: the board as text plus what it needs to plan the next move.
    pub fn report(&self) -> Value {
        let mut rows = Vec::new();
        let border: String = std::iter::repeat_n('#', self.size as usize + 2).collect();
        rows.push(border.clone());
        for y in 0..self.size {
            let mut r = String::from("#");
            for x in 0..self.size {
                let c = (x, y);
                r.push(if self.body.front() == Some(&c) {
                    self.heading.glyph()
                } else if self.body.contains(&c) {
                    'o'
                } else if self.food == Some(c) {
                    '*'
                } else {
                    '.'
                });
            }
            r.push('#');
            rows.push(r);
        }
        rows.push(border);
        let (hx, hy) = self.body.front().copied().unwrap_or((0, 0));
        json!({
            "running": self.alive,
            "last": self.last,
            "score": self.score,
            "best": self.best,
            "length": self.body.len(),
            "steps": self.steps,
            "heading": self.heading.name(),
            "head": { "x": hx, "y": hy },
            "food": self.food.map(|(x, y)| json!({ "x": x, "y": y })),
            "size": self.size,
            "mode": if self.tick_ms.is_some() { "real-time" } else { "turn-based" },
            "legend": "# wall, ^ > v < head (pointing where it's going), o body, * food, . empty; x grows right, y grows down",
            "board": rows,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_the_middle_heading_right() {
        let mut t = Training::seeded(1);
        t.start(16, None);
        assert!(t.alive() && t.visible());
        assert_eq!(t.body, [(8, 8), (7, 8), (6, 8)]);
        assert_eq!(t.heading, Heading::Right);
        assert!(t.food.is_some() && !t.body.contains(&t.food.unwrap()));
    }

    #[test]
    fn turns_are_relative_to_the_heading() {
        let mut t = Training::seeded(1);
        t.start(16, None);
        t.food = Some((0, 0));
        t.turn(Turn::Left);
        assert_eq!(t.step(), Step::Moved);
        assert_eq!(t.body[0], (8, 7));
        t.turn(Turn::Left);
        t.step();
        assert_eq!(t.body[0], (7, 7));
        t.turn(Turn::Right);
        t.step();
        assert_eq!(t.body[0], (7, 6));
    }

    #[test]
    fn eating_grows_by_one_and_scores() {
        let mut t = Training::seeded(2);
        t.start(16, None);
        t.food = Some((9, 8));
        assert_eq!(t.step(), Step::Ate);
        assert_eq!(t.body.len(), 4);
        assert_eq!(t.score, 1);
        assert_ne!(t.food, Some((9, 8)));
        assert_eq!(t.step(), Step::Moved);
        assert_eq!(t.body.len(), 4);
    }

    #[test]
    fn the_wall_ends_the_run() {
        let mut t = Training::seeded(3);
        t.start(8, None);
        t.food = Some((0, 0));
        let mut last = Step::Moved;
        for _ in 0..10 {
            last = t.step();
            if last != Step::Moved {
                break;
            }
        }
        assert_eq!(last, Step::Crashed);
        assert!(!t.alive());
        assert_eq!(t.step(), Step::Idle);
    }

    #[test]
    fn biting_itself_ends_the_run_but_the_tail_moves_away() {
        let mut t = Training::seeded(4);
        t.start(16, None);
        // Length 5, then a tight loop: right, right, right brings the head onto the body.
        t.body = [(8, 8), (7, 8), (6, 8), (5, 8), (4, 8)]
            .into_iter()
            .collect();
        t.food = Some((0, 0));
        t.turn(Turn::Right);
        assert_eq!(t.step(), Step::Moved); // (8,9)
        t.turn(Turn::Right);
        assert_eq!(t.step(), Step::Moved); // (7,9)
        t.turn(Turn::Right);
        assert_eq!(t.step(), Step::Crashed); // (7,8) is body
                                             // A length-4 loop chases its own tail and survives.
        let mut t = Training::seeded(4);
        t.start(16, None);
        t.body = [(8, 8), (7, 8), (7, 9), (8, 9)].into_iter().collect();
        t.heading = Heading::Right;
        t.food = Some((0, 0));
        t.turn(Turn::Right);
        assert_eq!(t.step(), Step::Moved); // onto (8,9), where the tail just left
    }

    #[test]
    fn real_time_turns_wait_for_the_tick_and_stale_tickers_stop() {
        let mut t = Training::seeded(5);
        t.start(16, Some(200));
        t.food = Some((0, 0));
        let g = t.generation();
        t.turn(Turn::Left);
        assert_eq!(t.heading, Heading::Right);
        assert!(t.tick(g));
        assert_eq!(t.body[0], (8, 7));
        t.start(16, Some(200));
        assert!(!t.tick(g));
    }

    #[test]
    fn the_report_draws_the_board() {
        let mut t = Training::seeded(6);
        t.start(8, None);
        t.food = Some((0, 0));
        let r = t.report();
        let board: Vec<String> = serde_json::from_value(r["board"].clone()).unwrap();
        assert_eq!(board.len(), 10);
        assert_eq!(board[1], "#*.......#");
        assert_eq!(board[5], "#..oo>...#");
    }
}
