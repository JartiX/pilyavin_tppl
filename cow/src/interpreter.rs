use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instruction {
    Moo = 0,   // moo - начало цикла
    MOo = 1,   // mOo - переместить указатель влево
    MoO = 2,   // moO - переместить указатель вправо
    MOO = 3,   // mOO - выполнить команду из текущей ячейки
    Moo2 = 4,  // Moo - вывести/ввести символ
    MOo2 = 5,  // MOo - декремент
    MoO2 = 6,  // MoO - инкремент
    MOO2 = 7,  // MOO - конец цикла
    OOO = 8,   // OOO - обнулить ячейку
    MMM = 9,   // MMM - работа с регистром
    OOM = 10,  // OOM - вывести число
    Oom = 11,  // oom - ввести число
}

pub struct CowInterpreter {
    pub program: Vec<Instruction>,
    pub memory: Vec<i32>,
    pub mem_pos: usize,
    pub prog_pos: usize,
    pub register: Option<i32>,
}

impl CowInterpreter {
    pub fn new(source: &str) -> Result<Self, String> {
        let program = Self::parse(source)?;
        Ok(CowInterpreter {
            program,
            memory: vec![0],
            mem_pos: 0,
            prog_pos: 0,
            register: None,
        })
    }

    fn parse(source: &str) -> Result<Vec<Instruction>, String> {
        let tokens = [
            ("moo", Instruction::Moo),
            ("mOo", Instruction::MOo),
            ("moO", Instruction::MoO),
            ("mOO", Instruction::MOO),
            ("Moo", Instruction::Moo2),
            ("MOo", Instruction::MOo2),
            ("MoO", Instruction::MoO2),
            ("MOO", Instruction::MOO2),
            ("OOO", Instruction::OOO),
            ("MMM", Instruction::MMM),
            ("OOM", Instruction::OOM),
            ("oom", Instruction::Oom),
        ];

        let mut program = Vec::new();
        let mut buffer = String::new();

        for ch in source.chars() {
            buffer.push(ch);
            if buffer.len() > 3 {
                buffer.remove(0);
            }

            if buffer.len() == 3 {
                for (token, instruction) in &tokens {
                    if buffer == *token {
                        program.push(*instruction);
                        buffer.clear();
                        break;
                    }
                }
            }
        }

        Ok(program)
    }

    pub fn execute(&mut self) -> Result<String, String> {
        let mut output = String::new();

        while self.prog_pos < self.program.len() {
            if !self.exec_instruction(&mut output)? {
                break;
            }
        }

        Ok(output)
    }

    pub fn execute_with_input(&mut self, input: &mut dyn Iterator<Item = String>) -> Result<String, String> {
        let mut output = String::new();

        while self.prog_pos < self.program.len() {
            if !self.exec_instruction_with_input(&mut output, input)? {
                break;
            }
        }

        Ok(output)
    }

    fn exec_instruction(&mut self, output: &mut String) -> Result<bool, String> {
        let mut stdin_iter = std::io::stdin().lines().map(|l| l.unwrap_or_default());
        self.exec_instruction_with_input(output, &mut stdin_iter)
    }

    pub fn exec_instruction_with_input(
        &mut self,
        output: &mut String,
        input: &mut dyn Iterator<Item = String>,
    ) -> Result<bool, String> {
        let instruction = self.program[self.prog_pos];

        match instruction {
            // moo - прыжок назад к предыдущему MOO
            Instruction::Moo => {
                if self.prog_pos == 0 {
                    return Ok(false);
                }

                self.prog_pos -= 1;
                let mut level = 1;

                while level > 0 {
                    if self.prog_pos == 0 {
                        break;
                    }
                    self.prog_pos -= 1;

                    if self.program[self.prog_pos] == Instruction::Moo {
                        level += 1;
                    } else if self.program[self.prog_pos] == Instruction::MOO2 {
                        level -= 1;
                    }
                }

                if level != 0 {
                    return Ok(false);
                }

                return self.exec_instruction_with_input(output, input);
            }

            // mOo - переместить указатель влево
            Instruction::MOo => {
                if self.mem_pos == 0 {
                    return Ok(false);
                }
                self.mem_pos -= 1;
            }

            // moO - переместить указатель вправо
            Instruction::MoO => {
                self.mem_pos += 1;
                if self.mem_pos >= self.memory.len() {
                    self.memory.push(0);
                }
            }

            // mOO - выполнить команду из текущей ячейки памяти
            Instruction::MOO => {
                let value = self.memory[self.mem_pos];
                if value == 3 {
                    return Ok(false);
                }
                if value >= 0 && value < 12 {
                    let saved_pos = self.prog_pos;
                    self.prog_pos = value as usize;
                    if self.prog_pos < self.program.len() {
                        self.exec_instruction_with_input(output, input)?;
                    }
                    self.prog_pos = saved_pos;
                } else {
                    return Ok(false);
                }
            }

            // Moo - вывести символ или ввести
            Instruction::Moo2 => {
                if self.memory[self.mem_pos] != 0 {
                    if let Some(ch) = char::from_u32(self.memory[self.mem_pos] as u32) {
                        output.push(ch);
                    }
                } else {
                    let input_str = input.next().unwrap_or_default();
                    if let Some(ch) = input_str.chars().next() {
                        self.memory[self.mem_pos] = ch as i32;
                    }
                }
            }

            // MOo - декремент
            Instruction::MOo2 => {
                self.memory[self.mem_pos] -= 1;
            }

            // MoO - инкремент
            Instruction::MoO2 => {
                self.memory[self.mem_pos] += 1;
            }

            // MOO - конец цикла (если ячейка == 0, прыгаем вперед)
            Instruction::MOO2 => {
                if self.memory[self.mem_pos] == 0 {
                    let mut level = 1;
                    self.prog_pos += 1;

                    if self.prog_pos >= self.program.len() {
                        return Ok(true);
                    }

                    let mut prev = self.program[self.prog_pos - 1];

                    while level > 0 {
                        prev = self.program[self.prog_pos];
                        self.prog_pos += 1;

                        if self.prog_pos >= self.program.len() {
                            break;
                        }

                        if self.program[self.prog_pos] == Instruction::MOO2 {
                            level += 1;
                        } else if self.program[self.prog_pos] == Instruction::Moo {
                            level -= 1;
                            if prev == Instruction::MOO2 {
                                level -= 1;
                            }
                        }
                    }

                    if level != 0 {
                        return Ok(false);
                    }
                }
            }

            // OOO - обнулить ячейку
            Instruction::OOO => {
                self.memory[self.mem_pos] = 0;
            }

            // MMM - работа с регистром
            Instruction::MMM => {
                if self.register.is_none() {
                    self.register = Some(self.memory[self.mem_pos]);
                } else {
                    self.memory[self.mem_pos] = self.register.unwrap();
                    self.register = None;
                }
            }

            // OOM - вывести число
            Instruction::OOM => {
                output.push_str(&self.memory[self.mem_pos].to_string());
                output.push('\n');
            }

            // oom - ввести число
            Instruction::Oom => {
                let input_str = input.next().unwrap_or_default();
                self.memory[self.mem_pos] = input_str.trim().parse().unwrap_or(0);
            }
        }

        self.prog_pos += 1;
        Ok(true)
    }

    pub fn get_memory(&self) -> &[i32] {
        &self.memory
    }

    pub fn get_memory_pos(&self) -> usize {
        self.mem_pos
    }

    pub fn get_register(&self) -> Option<i32> {
        self.register
    }
}
