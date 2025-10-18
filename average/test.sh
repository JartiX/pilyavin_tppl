#!/bin/bash

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

TESTS_PASSED=0
TESTS_FAILED=0

run_test() {
    local test_name=$1
    local expected=$2
    local data_content=$3
    
    echo -e "${YELLOW}Тест: $test_name${NC}"
    
    echo -e "$data_content" > data.txt
    
    result=$(./main 2>&1)
    
    actual=$(echo "$result" | sed -n 's/.*Result: \(-*[0-9]*\).*/\1/p')
    actual=$(echo "$actual" | xargs)
    
    if [ "$actual" == "$expected" ]; then
        echo -e "${GREEN} PASSED${NC} (ожидалось: $expected, получено: $actual)"
        ((TESTS_PASSED++))
    else
        echo -e "${RED} FAILED${NC} (ожидалось: $expected, получено: $actual)"
        ((TESTS_FAILED++))
    fi
    echo ""
}

echo "Компиляция программы..."
nasm -f elf64 main.asm -o main.o
if [ $? -ne 0 ]; then
    echo -e "${RED}Ошибка компиляции NASM${NC}"
    exit 1
fi

ld main.o -o main
if [ $? -ne 0 ]; then
    echo -e "${RED}Ошибка линковки${NC}"
    exit 1
fi

echo -e "${GREEN}Компиляция успешна${NC}\n"

run_test "Базовый тест" \
    "-1" \
    "5 3 2 6 1 7 4\n0 10 1 9 2 8 5"

run_test "Положительный результат" \
    "4" \
    "5 6 7 8\n1 2 3 4"

run_test "Нулевой результат" \
    "0" \
    "1 2 3 4\n1 2 3 4"

run_test "Все отрицательные разности" \
    "-3" \
    "1 2 3\n4 5 6"

run_test "Большие числа" \
    "500" \
    "1000 2000 3000\n500 1500 2500"

run_test "Отрицательные числа в массивах" \
    "0" \
    "-5 -3 2\n-5 -3 2"

run_test "Один элемент" \
    "10" \
    "15\n5"

run_test "Два элемента" \
    "5" \
    "10 20\n5 15"

run_test "Формат с запятыми" \
    "-1" \
    "5, 3, 2, 6, 1, 7, 4\n0, 10, 1, 9, 2, 8, 5"

run_test "Смешанный формат" \
    "10" \
    "10, 20 30\n5, 10 15"

run_test "Отрицательные числа" \
    "5" \
    "5 -5 10\n0 -10 5"

run_test "Большое количество элементов" \
    "0" \
    "1 2 3 4 5 6 7 8 9 10\n1 2 3 4 5 6 7 8 9 10"

run_test "Максимальная разница" \
    "1000" \
    "2000 3000\n1000 2000"

run_test "Дробный результат" \
    "5" \
    "10 11\n5 5"

run_test "Отрицательный дробный результат" \
    "-5" \
    "5 5\n10 11"

run_test "Лишние пробелы и переносы" \
    "10" \
    "10   20    30\n5   10    15"

run_test "Нулевые значения" \
    "0" \
    "0 0 0\n0 0 0"

run_test "Чередование знаков" \
    "0" \
    "5 -5 10 -10\n5 -5 10 -10"

echo -e "${YELLOW}Тест: Отсутствие файла data.txt${NC}"
rm -f data.txt
./main > /dev/null 2>&1
exit_code=$?
if [ $exit_code -ne 0 ]; then
    echo -e "${GREEN}PASSED${NC} (программа корректно обработала отсутствие файла)"
    ((TESTS_PASSED++))
else
    echo -e "${RED}FAILED${NC} (программа должна завершиться с ошибкой)"
    ((TESTS_FAILED++))
fi
echo ""

echo "======================================"
echo -e "Всего тестов: $((TESTS_PASSED + TESTS_FAILED))"
echo -e "${GREEN}Пройдено: $TESTS_PASSED${NC}"
echo -e "${RED}Провалено: $TESTS_FAILED${NC}"
echo "======================================"

rm -f main.o main data.txt

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}Все тесты пройдены успешно!${NC}"
    exit 0
else
    echo -e "${RED}Некоторые тесты провалены${NC}"
    exit 1
fi