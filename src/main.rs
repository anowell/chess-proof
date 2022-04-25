use anyhow::Result;
use chess::{BitBoard, Board, ChessMove, Color, MoveGen, Piece, Rank, Square};
use rayon::prelude::*;
use std::collections::HashSet;
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

static SOLUTION_COUNT: AtomicU64 = AtomicU64::new(0);
static BOARD_COUNT: AtomicU64 = AtomicU64::new(0);
static FINAL_FEN_PREFIX: &str = "4k3/8/8/8/8/8/PPPPPPPP/RNBQKBNR";
type SharedPathSet = Arc<Mutex<HashSet<[Square; 16]>>>;

macro_rules! cm {
    ($src:tt, $dest:tt) => {
        chess::ChessMove::new(chess::Square::$src, chess::Square::$dest, None)
    };
}

fn main() -> Result<()> {
    // To check if we've reached the solution, we compare with  BitBoard of all the pieces
    // Technically, this could allow scenarios where a white rook captures a black knight on B2 or G2
    // So we need an additionl check, but choosing a fast check helps significantly with perf
    let solution = chess::get_rank(Rank::First)
        | chess::get_rank(Rank::Second)
        | BitBoard::from_square(Square::E8);

    let mut board = Board::default();
    let mut opening = vec![
        cm!(B1, C3), // 1
        cm!(B7, B5),
        cm!(C3, B5), // 2
        cm!(G8, F6),
        cm!(B5, A7), // 3
        cm!(F6, E4),
        cm!(A7, C8), // 4
        cm!(E4, C3),
        cm!(C8, E7), // 5
        cm!(G7, G6),
        cm!(E7, G6), // 6
        cm!(C3, B1),
        cm!(G6, H8), // 7
                     // cm!(D8, G5),
                     // cm!(H8, F7),  // 8
                     // cm!(C7, C5),
                     // cm!(F7, G5),  // 9
                     // cm!(A8, A4),
                     // cm!(G5, H7),  // 10
                     // cm!(B8, C6),
                     // cm!(H7, F8),  // 11
                     // cm!(C6, B4),
                     // cm!(F8, D7),  // 12
                     // cm!(B4, D5),
                     // cm!(D7, C5),  // 13
                     // cm!(D5, C3),
                     // cm!(C5, A4),  // 14
                     // cm!(E8, F8),
                     // cm!(A4, C3),  // 15
                     // cm!(F8, E8),
    ];
    opening.reserve(32 - opening.len());

    for mv in &opening {
        board = board.make_move_new(*mv);
    }
    pretty_board(board);

    let knight_paths: SharedPathSet = Arc::new(Mutex::new(HashSet::new()));
    let knight_paths_monitor = knight_paths.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(10));
        println!(
            "Processed {} positions; {} solutions; {} unique paths",
            pretty_num(BOARD_COUNT.load(Ordering::Relaxed)),
            pretty_num(SOLUTION_COUNT.load(Ordering::Relaxed)),
            pretty_num(knight_paths_monitor.lock().unwrap().len() as u64),
        );
    });

    process_board(board, opening, solution, knight_paths.clone());

    println!("----------");
    println!(
        "Processed {} positions",
        pretty_num(BOARD_COUNT.load(Ordering::Relaxed))
    );
    println!(
        "Found {} solutions",
        pretty_num(SOLUTION_COUNT.load(Ordering::Relaxed))
    );

    let paths = knight_paths
        .lock()
        .expect("Failed to acquire knight_paths lock");
    println!(
        "Found {} unique knight paths",
        pretty_num(paths.len() as u64)
    );

    for path in paths.iter() {
        for square in path {
            print!("{} ", square);
        }
        println!("")
    }

    Ok(())
}

fn process_board(
    board: Board,
    move_list: Vec<ChessMove>,
    solution: BitBoard,
    knight_paths: SharedPathSet,
) {
    assert!(
        move_list.len() > 0,
        "Must at least specify the first move, otherwise it is ambiguous which knight to use"
    );
    match board.side_to_move() {
        Color::White => {
            let knight_square = move_list[move_list.len() - 2].get_dest();
            white_to_move(board, move_list, knight_square, solution, knight_paths);
        }
        Color::Black => {
            let knight_square = move_list[move_list.len() - 1].get_dest();
            black_to_move(board, move_list, knight_square, solution, knight_paths);
        }
    }
}

fn white_to_move(
    board: Board,
    move_list: Vec<ChessMove>,
    knight_square: Square,
    solution: BitBoard,
    knight_paths: SharedPathSet,
) {
    let checkers = board.checkers();

    // double-check means we can't make a knight capture
    if checkers.popcnt() > 1 {
        return;
    }

    let knight_dests = if checkers != &chess::EMPTY {
        // moves where the knight would capture the checker
        chess::get_knight_moves(knight_square) & board.checkers()
    } else {
        // any valid knight move that captures a black piece
        chess::get_knight_moves(knight_square)
            & board.color_combined(Color::Black)
            & !BitBoard::from_square(board.king_square(Color::Black))
    };

    knight_dests.into_iter().for_each(|knight_dest| {
        let white_move = ChessMove::new(knight_square, knight_dest, None);
        if !board.legal(white_move) {
            println!("Illegal knight capture move: {}", white_move);
            pretty_board(board);
            panic!("Illegal knight move");
        }

        let knight_paths = knight_paths.clone();
        let board = board.make_move_new(white_move);
        let mut ml = move_list.clone();
        ml.push(white_move);

        BOARD_COUNT.fetch_add(1, Ordering::Relaxed);
        // faster to check number of black pieces remining than hash
        if *board.combined() == solution && board.to_string().starts_with(FINAL_FEN_PREFIX) {
            let _ = SOLUTION_COUNT.fetch_add(1, Ordering::Relaxed);
            // print_move_list(&ml);
            let path: [Square; 16] = ml
                .chunks(2)
                .map(|pair| pair[0].get_dest())
                .collect::<Vec<_>>()
                .try_into()
                .expect("length of move list didn't match");
            let mut paths = knight_paths.lock().expect("Mutex lock failure");
            let _ = paths.insert(path);
            return;
        }

        black_to_move(board, ml, white_move.get_dest(), solution, knight_paths);
    });
}

fn black_to_move(
    board: Board,
    move_list: Vec<ChessMove>,
    knight_square: Square,
    solution: BitBoard,
    knight_paths: SharedPathSet,
) {
    // Black can make any legal, non-capturing move
    let mut black_move_gen = MoveGen::new_legal(&board);
    let non_capture_mask = !board.color_combined(Color::White);
    black_move_gen.set_iterator_mask(non_capture_mask);

    // Parellel iteration using Rayon
    // This is where exponential growth of the search space happens
    // Hence I experimented with a few simple filters that try to reduce what moves get considered
    black_move_gen
        .into_iter()
        // Don't allow moving backward more than 1 square (which still allows for better knight repositioning)
        .filter(|black_move| {
            black_move.get_source().get_rank().to_index() + 1
                > black_move.get_dest().get_rank().to_index()
        })
        // Only allow the king to move on the last 2 moves
        .filter(|black_move| move_list.len() > 28 || black_move.get_source() != Square::E8)
        // reduce search space by ignoring backward moves from black
        // .filter(|black_move| black_move.get_source().get_rank() > black_move.get_dest().get_rank())
        // Never allow the king to move
        // .filter(|black_move| black_move.get_source() != Square::E8)
        .par_bridge()
        .for_each(|black_move| {
            let knight_paths = knight_paths.clone();
            let board = board.make_move_new(black_move);

            // Faster to copy_nonoverlappy like this than clone and push (based on flamegraph results)
            // let mut ml = move_list.clone();
            let mut ml = Vec::with_capacity(32);
            let (src, dest) = (move_list.as_ptr(), ml.as_mut_ptr());
            unsafe {
                std::ptr::copy_nonoverlapping(src, dest, move_list.len());
                ml.set_len(move_list.len());
            }
            ml.push(black_move);

            BOARD_COUNT.fetch_add(1, Ordering::Relaxed);
            if *board.combined() == solution && board.to_string().starts_with(FINAL_FEN_PREFIX) {
                let _ = SOLUTION_COUNT.fetch_add(1, Ordering::Relaxed);
                // print_move_list(&ml);
                let path: [Square; 16] = ml
                    .chunks(2)
                    .map(|pair| pair[0].get_dest())
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("length of move list didn't match");
                let mut paths = knight_paths.lock().expect("Mutex lock failure");
                let _ = paths.insert(path);
                return;
            }

            white_to_move(board, ml, knight_square, solution, knight_paths);
        });
}

#[allow(dead_code)]
fn print_move_list(move_list: &[ChessMove]) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    for (i, mv) in move_list.chunks(2).enumerate() {
        let _ = write!(
            &mut lock,
            "{}. {} {} ",
            i + 1,
            mv[0],
            mv.get(1)
                .map(ChessMove::to_string)
                .unwrap_or_else(String::new)
        );
    }
    let _ = writeln!(&mut lock);
}

fn pretty_board(board: Board) {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    for rank in (0..8).rev() {
        for file in 0..8 {
            let square = unsafe { Square::new(rank * 8 + file) };
            match board.piece_on(square) {
                None => {
                    let _ = write!(&mut lock, ". ");
                }
                Some(piece) => {
                    let _ = match (board.color_on(square).unwrap(), piece) {
                        (Color::White, Piece::King) => write!(&mut lock, "♔ "),
                        (Color::White, Piece::Queen) => write!(&mut lock, "♕ "),
                        (Color::White, Piece::Rook) => write!(&mut lock, "♖ "),
                        (Color::White, Piece::Bishop) => write!(&mut lock, "♗ "),
                        (Color::White, Piece::Knight) => write!(&mut lock, "♘ "),
                        (Color::White, Piece::Pawn) => write!(&mut lock, "♙ "),
                        (Color::Black, Piece::King) => write!(&mut lock, "♚ "),
                        (Color::Black, Piece::Queen) => write!(&mut lock, "♛ "),
                        (Color::Black, Piece::Rook) => write!(&mut lock, "♜ "),
                        (Color::Black, Piece::Bishop) => write!(&mut lock, "♝ "),
                        (Color::Black, Piece::Knight) => write!(&mut lock, "♞ "),
                        (Color::Black, Piece::Pawn) => write!(&mut lock, "♟ "),
                    };
                }
            }
        }
        let _ = writeln!(&mut lock, "");
    }
}

fn pretty_num(num: u64) -> String {
    if num / 1_000_000_000 > 0 {
        format!("{:.1}B", num as f64 / 1_000_000_000.0)
    } else if num / 1_000_000 > 0 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num / 1_000 > 0 {
        format!("{:.1}k", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}
