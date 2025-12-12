use pascal_interpreter::execute;

fn main() {
    let program = r#"
        BEGIN
            x := 2;
            y := 3;
            z := x + y * 2;
            result := (x + y) * (z - 1)
        END.
    "#;

    match execute(program) {
        Ok(variables) => {
            println!("Program executed successfully!");
            println!("Variables:");
            let mut vars: Vec<_> = variables.iter().collect();
            vars.sort_by_key(|(name, _)| *name);
            for (name, value) in vars {
                println!("  {} = {}", name, value);
            }
        }
        Err(err) => {
            eprintln!("Error: {}", err);
        }
    }
}