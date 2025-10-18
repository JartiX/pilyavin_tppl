section .data
    filename db "data.txt", 0
    msg db "Result: ", 0
    error_msg db "Error", 10
    error_len equ $ - error_msg

section .bss
    x resd 100
    y resd 100
    len resd 1
    buffer resb 1024
    fd resd 1

section .text
    global _start

_start:
    ; Открытие и чтение файла
    mov rax, 2              ; sys_open
    mov rdi, filename
    xor rsi, rsi            ; O_RDONLY
    xor rdx, rdx            ; mode
    syscall
    test rax, rax
    js .error_exit          ; Переходим на ошибку с выходом
    
    mov [fd], eax
    mov edi, eax
    mov rax, 0              ; sys_read
    mov rsi, buffer
    mov rdx, 1024
    syscall
    
    mov r15, rax            ; Длина прочитанных данных
    
    mov rax, 3              ; sys_close
    mov edi, [fd]
    syscall
    
    ; Парсинг чисел
    xor r8, r8              ; Индекс в буфере
    xor r9, r9              ; Счетчик чисел для массива x
    xor r10, r10            ; Счетчик чисел для массива y
    xor r11, r11            ; Флаг: 0 = первый массив, 1 = второй массив
    
.parse_loop:
    cmp r8, r15
    jge .parse_done
    
    movzx edx, byte [buffer + r8]
    inc r8
    
    ; Пропускаем пробелы, запятые и переносы строк
    cmp dl, ' '
    je .parse_loop
    cmp dl, ','
    je .parse_loop
    cmp dl, 10              ; Новая строка
    je .next_array
    cmp dl, 13              ; Возврат каретки
    je .parse_loop
    
    ; Нашли начало числа
    dec r8                  ; Возвращаемся к первому символу числа
    
    ; Парсим число
    xor rax, rax            ; Текущее число
    xor rcx, rcx            ; Знак (0=+, 1=-)
    xor r12, r12            ; Флаг, что число началось
    
    ; Проверяем знак
    movzx edx, byte [buffer + r8]
    cmp dl, '-'
    jne .parse_digits
    mov rcx, 1
    inc r8
    
.parse_digits:
    cmp r8, r15
    jge .save_number
    
    movzx edx, byte [buffer + r8]
    
    cmp dl, '0'
    jb .save_number
    cmp dl, '9'
    ja .save_number
    
    sub dl, '0'
    imul rax, 10
    add rax, rdx
    mov r12, 1              ; Число началось
    inc r8
    jmp .parse_digits

.save_number:
    test r12, r12           ; Было ли вообще число?
    jz .parse_loop
    
    ; Применяем знак
    test rcx, rcx
    jz .positive
    neg rax
.positive:
    
    ; Сохраняем в соответствующий массив
    test r11, r11
    jnz .save_to_y
    
    ; Сохраняем в массив x
    cmp r9, 100
    jge .parse_loop         ; Превышен лимит массива
    mov [x + r9*4], eax
    inc r9
    jmp .parse_loop
    
.save_to_y:
    ; Сохраняем в массив y
    cmp r10, 100
    jge .parse_loop         ; Превышен лимит массива
    mov [y + r10*4], eax
    inc r10
    jmp .parse_loop

.next_array:
    test r11, r11
    jnz .parse_loop         ; Уже во втором массиве, продолжаем
    mov r11, 1              ; Переходим ко второму массиву
    jmp .parse_loop

.parse_done:
    ; Проверяем, что оба массива имеют одинаковую длину
    cmp r9, r10
    jne .error
    
    mov [len], r9d
    
    ; Если нет чисел, выходим
    test r9, r9
    jz .error
    
    ; Вычисление среднего разностей
    xor rax, rax            ; Сумма
    xor rcx, rcx            ; Индекс
    
.calc_loop:
    cmp ecx, [len]
    jge .calc_avg
    
    ; Вычисляем x[i] - y[i]
    mov ebx, [x + rcx*4]
    sub ebx, [y + rcx*4]
    movsxd rbx, ebx
    add rax, rbx
    
    inc ecx
    jmp .calc_loop

.calc_avg:
    movsx rbx, dword [len]
    cqo
    idiv rbx
    
    ; Вывод результата
    push rax
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, 8
    syscall
    pop rax
    
    ; Преобразование числа в строку
    mov rdi, buffer + 1023  ; Конец буфера
    mov byte [rdi], 0       ; Null terminator
    dec rdi
    mov byte [rdi], 10      ; Новая строка
    dec rdi
    
    mov rbx, rax            ; Сохраняем исходное число
    test rax, rax
    jns .convert_positive
    neg rax                 ; Делаем положительным для конвертации

.convert_positive:
    mov rcx, 10             ; Основание системы

.convert_loop:
    xor rdx, rdx
    div rcx
    add dl, '0'
    mov [rdi], dl
    dec rdi
    test rax, rax
    jnz .convert_loop
    
    test rbx, rbx
    jns .output
    mov byte [rdi], '-'
    dec rdi

.output:
    inc rdi                 ; Корректируем указатель на начало строки
    mov rsi, rdi
    mov rdx, buffer + 1024
    sub rdx, rdi            ; Длина строки
    
    mov rax, 1
    mov rdi, 1
    syscall
    jmp .success_exit

.error:
    ; Вывод сообщения об ошибке
    mov rax, 1
    mov rdi, 1
    mov rsi, msg
    mov rdx, 8
    syscall
    
    mov rax, 1
    mov rdi, 1
    mov rsi, error_msg
    mov rdx, error_len
    syscall
    jmp .error_exit

.error_exit:                ; Выход с кодом ошибки
    mov rax, 60
    mov rdi, 1              ; Код возврата 1 (ошибка)
    syscall

.success_exit:              ; Успешный выход
    mov rax, 60
    xor rdi, rdi            ; Код возврата 0 (успех)
    syscall