use cow_interpreter::interpreter::CowInterpreter;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Использование: {} <файл>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    let source = fs::read_to_string(filename).unwrap_or_else(|err| {
        eprintln!("Ошибка при чтении файла '{}': {}", filename, err);
        process::exit(1);
    });

    let mut interpreter = CowInterpreter::new(&source).unwrap_or_else(|err| {
        eprintln!("Ошибка при разборе программы: {}", err);
        process::exit(1);
    });

    match interpreter.execute() {
        Ok(output) => {
            if output.is_empty() {
                println!("Программа выполнена, но вывода нет.");
            } else {
                print!("{}", output);
            }
        }
        Err(err) => {
            eprintln!("Ошибка при выполнении программы: {}", err);
            process::exit(1);
        }
    }
}