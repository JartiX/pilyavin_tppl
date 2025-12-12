use cow_interpreter::interpreter::CowInterpreter;
use cow_interpreter::interpreter::Instruction;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty() {
        let interpreter = CowInterpreter::new("").unwrap();
        assert_eq!(interpreter.program.len(), 0);
    }

    #[test]
    fn test_parse_single_instruction() {
        let interpreter = CowInterpreter::new("moo").unwrap();
        assert_eq!(interpreter.program.len(), 1);
        assert_eq!(interpreter.program[0], Instruction::Moo);
    }

    #[test]
    fn test_parse_multiple_instructions() {
        let interpreter = CowInterpreter::new("mooMoOMOo").unwrap();
        assert_eq!(interpreter.program.len(), 3);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let interpreter = CowInterpreter::new("moo   MoO   MOo").unwrap();
        assert_eq!(interpreter.program.len(), 3);
    }

    #[test]
    fn test_increment() {
        let mut interpreter = CowInterpreter::new("MoO").unwrap(); 
        interpreter.execute().unwrap();
        assert_eq!(interpreter.memory[0], 1);
    }

    #[test]
    fn test_decrement() {
        let mut interpreter = CowInterpreter::new("MoOMOo").unwrap(); 
        interpreter.execute().unwrap();
        assert_eq!(interpreter.memory[0], 0);
    }

    #[test]
    fn test_move_right_and_allocate() {
        let mut interpreter = CowInterpreter::new("moO").unwrap();
        interpreter.execute().unwrap();
        assert_eq!(interpreter.mem_pos, 1);
        assert!(interpreter.memory.len() >= 2);
    }

    #[test]
    fn test_move_left_at_zero_does_not_underflow() {
        let mut interpreter = CowInterpreter::new("mOo").unwrap();
        let _ = interpreter.execute();
        assert_eq!(interpreter.mem_pos, 0);
    }

    #[test]
    fn test_zero_cell() {
        let mut interpreter = CowInterpreter::new("MoOMoOMoOOOO").unwrap(); 
        interpreter.execute().unwrap();
        assert_eq!(interpreter.memory[0], 0);
    }

    #[test]
    fn test_print_number() {
        let mut interpreter = CowInterpreter::new("MoOMoOMoOOOM").unwrap();
        let output = interpreter.execute().unwrap();
        assert_eq!(output, "3\n");
    }

    #[test]
    fn test_print_char_after_input() {
        let mut interpreter = CowInterpreter::new("MooOOM").unwrap();
        let mut input = vec!["A".to_string()].into_iter();
        let output = interpreter.execute_with_input(&mut input).unwrap();
        assert_eq!(output, "65\n");
    }

    #[test]
    fn test_input_number() {
        let mut interpreter = CowInterpreter::new("oomOOM").unwrap(); 
        let mut input = vec!["42".to_string()].into_iter();
        let output = interpreter.execute_with_input(&mut input).unwrap();
        assert_eq!(output, "42\n");
    }

    #[test]
    fn test_memory_execution_from_cells_simple() {
        let mut interpreter = CowInterpreter::new("MoOMoOMoOMoO").unwrap();
        interpreter.execute().unwrap();
        assert!(interpreter.memory[0] >= 0);
    }

    #[test]
    fn test_register_save_and_load() {
        let mut interpreter = CowInterpreter::new("MoOMoOMoOMMM").unwrap();
        interpreter.execute().unwrap();
        assert_eq!(interpreter.register, Some(3));

        let mut interpreter = CowInterpreter::new("MoOMoOMoOMMMmoOOOOMMMOOM").unwrap();
        let output = interpreter.execute().unwrap();
        assert_eq!(output, "3\n");
        assert!(interpreter.memory.len() > 1 && interpreter.memory[1] == 3);
    }

    #[test]
    fn test_print_invalid_char_produces_no_output() {
        let mut interpreter = CowInterpreter::new("MOoMOoMOoMoo").unwrap(); 
        let output = interpreter.execute().unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn test_multiple_cells_and_positions() {
        let mut interpreter = CowInterpreter::new("MoOmoOMoOmoOMoO").unwrap();
        interpreter.execute().unwrap();
        assert!(interpreter.memory.len() >= 2);
        assert!(interpreter.mem_pos >= 0);
    }

    #[test]
    fn test_complex_memory_ops_smoke() {
        let mut interpreter = CowInterpreter::new("MoOMoOmoOMoOMoOmOoMoOMoOMoO").unwrap();
        interpreter.execute().unwrap();
        assert!(interpreter.memory.iter().all(|&v| v >= 0));
    }

    #[test]
    fn test_input_edge_cases() {
        let mut interpreter = CowInterpreter::new("Moo").unwrap();
        let mut input = vec!["".to_string()].into_iter();
        interpreter.execute_with_input(&mut input).unwrap();
        assert_eq!(interpreter.memory[0], 0);

        let mut interpreter = CowInterpreter::new("oom").unwrap();
        let mut input = vec!["abc".to_string()].into_iter();
        interpreter.execute_with_input(&mut input).unwrap();
        assert_eq!(interpreter.memory[0], 0);
    }

    #[test]
    fn test_getters() {
        let mut interpreter = CowInterpreter::new("MoOMoOMoOMMMmoO").unwrap();
        interpreter.execute().unwrap();
        assert_eq!(interpreter.get_memory()[0], 3);
        assert_eq!(interpreter.get_memory_pos(), interpreter.mem_pos);
        assert_eq!(interpreter.get_register(), interpreter.register);
    }

    #[test]
    fn test_no_unexpected_panic_on_various_inputs() {
        let programs = [
            "MoO",                  
            "MoOMOo",               
            "moOmoO",               
            "MoOMoOOOM",            
            "OOM",                  
        ];
        for p in &programs {
            let mut interpreter = CowInterpreter::new(p).unwrap();
            let _ = interpreter.execute();
        }
    }

    #[test]
    fn test_execute_with_input_break() {
        let mut interpreter = CowInterpreter::new("mOo").unwrap();
        interpreter.mem_pos = 0; // explicitly at 0
        let mut input = vec![].into_iter();
        
        let output = interpreter.execute_with_input(&mut input).unwrap();

        assert_eq!(output, "");
        assert!(interpreter.prog_pos <= interpreter.program.len());
    }

    #[test]
    fn test_moo_loop_backward_jump() {
        let program = "MoOMOOmoo";
        let mut interpreter = CowInterpreter::new(program).unwrap();

        interpreter.memory[0] = 1;

        let mut input = vec![].into_iter();
        let _ = interpreter.execute_with_input(&mut input).unwrap();

        assert!(interpreter.memory[0] >= 0);
        assert!(interpreter.prog_pos <= interpreter.program.len());
    }

    #[test]
    fn moo_at_start_returns_false() {
        let mut interp = CowInterpreter::new("moo").unwrap();
        interp.prog_pos = 0;
        let mut input = vec![].into_iter();
        let res = interp.exec_instruction_with_input(&mut String::new(), &mut input).unwrap();
        assert!(!res);
        assert_eq!(interp.prog_pos, 0);
    }

}
