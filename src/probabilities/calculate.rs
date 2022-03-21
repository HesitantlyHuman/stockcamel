use crate::board;
use crate::board::Board;
use crate::constants;
use crate::probabilities::accumulators::{
    AtomicPositionAccumulator, AtomicTileAccumulator, PositionAccumulator, TileAccumulator,
};
use crate::probabilities::odds::{CamelOdds, TileOdds};
use crossbeam::queue::ArrayQueue;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use std::sync::{Arc, Mutex};
use std::{panic, thread};

pub fn solve_probabilities(
    board: board::Board,
    depth: u8,
    num_workers: usize,
) -> (CamelOdds, CamelOdds, TileOdds) {
    coz::scope!("Solve Probabilities");
    let round_positions_accumulator = AtomicPositionAccumulator::new();
    let game_positions_accumulator = AtomicPositionAccumulator::new();
    let tile_accumulator = AtomicTileAccumulator::new();

    let stack = ArrayQueue::new(num_workers * 2);
    let _ = stack.push((board, depth));
    seed_stack(&stack, num_workers);

    let transition_depth = depth - board.num_unrolled();

    (0..num_workers).into_par_iter().for_each(|_| {
        coz::thread_init();
        start_worker(
            &stack,
            transition_depth,
            &game_positions_accumulator,
            &round_positions_accumulator,
            &tile_accumulator,
        );
    });

    let round_positions_accumulator: PositionAccumulator = round_positions_accumulator.into();
    let game_positions_accumulator: PositionAccumulator = game_positions_accumulator.into();
    let tile_accumulator: TileAccumulator = tile_accumulator.into();

    let round_terminal_states = round_positions_accumulator.count_terminal();
    let round_position_odds = CamelOdds::new(&round_positions_accumulator, &round_terminal_states);
    let game_position_odds = game_positions_accumulator.into();
    let tile_odds = TileOdds::new(&tile_accumulator, &round_terminal_states);
    (game_position_odds, round_position_odds, tile_odds)
}

fn start_worker(
    stack: &ArrayQueue<(Board, u8)>,
    transition_depth: u8,
    game_positions_accumulator: &AtomicPositionAccumulator,
    round_positions_accumulator: &AtomicPositionAccumulator,
    tile_accumulator: &AtomicTileAccumulator,
) {
    let mut private_game_positions = PositionAccumulator::new();
    let mut private_round_positions = PositionAccumulator::new();
    let mut private_tile_positions = TileAccumulator::new();
    loop {
        let (board, depth) = match stack.pop() {
            Some((board, depth)) => (board, depth),
            None => break,
        };
        if depth > transition_depth {
            let (game_accumulations, round_accumulations, tile_accumulations) =
                calculate_round_and_game_terminal_states(&board, &depth, &transition_depth);
            private_game_positions += game_accumulations;
            private_round_positions += round_accumulations;
            private_tile_positions += tile_accumulations;
        } else {
            let game_accumulations = calculate_game_terminal_states(&board, &depth);
            private_game_positions += game_accumulations;
        }
    }
    game_positions_accumulator.add(private_game_positions);
    round_positions_accumulator.add(private_round_positions);
    tile_accumulator.add(private_tile_positions);
}

fn calculate_round_and_game_terminal_states(
    board: &Board,
    depth: &u8,
    transition_depth: &u8,
) -> (PositionAccumulator, PositionAccumulator, TileAccumulator) {
    if depth == &0 {
        let accum = terminal_node_heuristic(board).into();
        return (accum, accum, TileAccumulator::new());
    } else if board.is_terminal() {
        let accum = board.camel_order().into();
        return (accum, accum, TileAccumulator::new());
    }

    let mut game_positions_accumulator = PositionAccumulator::new();
    let mut round_positions_accumulator = PositionAccumulator::new();
    let mut tile_accumulator = TileAccumulator::new();

    if depth <= transition_depth {
        round_positions_accumulator += board.camel_order().into();
        let game_positions = calculate_game_terminal_states(board, depth);
        game_positions_accumulator += game_positions;
        return (
            game_positions_accumulator,
            round_positions_accumulator,
            tile_accumulator,
        );
    }

    for roll in board.potential_moves() {
        let next_board = board.update(&roll);
        let (game_positions, round_positions, tiles) =
            calculate_round_and_game_terminal_states(&next_board, &(depth - 1), transition_depth);
        game_positions_accumulator += game_positions;
        round_positions_accumulator += round_positions;
        tile_accumulator += tiles;
    }
    return (
        game_positions_accumulator,
        round_positions_accumulator,
        tile_accumulator,
    );
}

fn calculate_game_terminal_states(board: &Board, depth: &u8) -> PositionAccumulator {
    if depth == &0 {
        return terminal_node_heuristic(board).into();
    } else if board.is_terminal() {
        return board.camel_order().into();
    }

    let mut positions_accumulator = PositionAccumulator::new();

    for roll in board.potential_moves() {
        let next_board = board.update(&roll);
        let positions = calculate_game_terminal_states(&next_board, &(depth - 1));
        positions_accumulator += positions;
    }
    return positions_accumulator;
}

fn seed_stack(stack: &ArrayQueue<(Board, u8)>, num_to_seed: usize) {
    let mut num_seeded = stack.len();
    while num_seeded < num_to_seed {
        let (board, depth) = match stack.pop() {
            Some((board, depth)) => (board, depth),
            None => panic!(
                "Failed to seed the stack with at least {} board states!",
                num_seeded
            ),
        };
        num_seeded -= 1;
        for roll in board.potential_moves() {
            let next_board = board.update(&roll);
            match stack.push((next_board, depth - 1)) {
                Ok(_) => {}
                Err(_) => panic!("Exceeded probability stack!"),
            };
            num_seeded += 1;
        }
    }
}

fn terminal_node_heuristic(board: &board::Board) -> board::CamelOrder {
    return board.camel_order();
}

fn terminal_round_states_from_board(board: board::Board) -> u32 {
    let num_unrolled = board.num_unrolled() as u32;
    return num_unrolled.pow(constants::MAX_ROLL as u32);
}
