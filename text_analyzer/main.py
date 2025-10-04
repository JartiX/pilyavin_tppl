import argparse
import os

def analyze_text(file_path):
    if not os.path.exists(file_path):
        print(f"File not found: {file_path}")
        return None
    
    with open(file_path, 'r', encoding='utf-8') as file:
        text = file.read()

    num_chars = len(text)
    num_empty_lines = text.count('\n\n')
    num_lines = text.count('\n') + 1

    char_freq = {}
    for char in text:
        char_freq[char] = char_freq.get(char, 0) + 1

    return {
        "chars": num_chars,
        "lines": num_lines,
        "empty_lines": num_empty_lines,
        "freq": char_freq
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze text file for character and line statistics.")
    parser.add_argument("file_path", type=str, help="Path to the text file to analyze.")
    parser.add_argument("--count-lines", action="store_true", help="Count lines in the text.")
    parser.add_argument("--count-chars", action="store_true", help="Count characters in the text.")
    parser.add_argument("--count-empty-lines", action="store_true", help="Count empty lines in the text.")
    parser.add_argument("--count-freq", action="store_true", help="Count character frequency in the text.")
    
    args = parser.parse_args()

    stats = analyze_text(args.file_path)
    
    if args.count_lines:
        print(f"Number of lines: {stats['lines']}")

    if args.count_chars:
        print(f"Number of characters: {stats['chars']}")

    if args.count_empty_lines:
        print(f"Number of empty lines: {stats['empty_lines']}")

    if args.count_freq:
        print("Character frequency:")
        for char, freq in stats["freq"].items():
            print(f"'{char}': {freq}")

    if not any([args.count_lines, args.count_chars, args.count_empty_lines, args.count_freq]):
        print(f"Number of characters: {stats['chars']}")
        print(f"Number of lines: {stats['lines']}")
        print(f"Number of empty lines: {stats['empty_lines']}")
        print("Character frequency:")
        for char, freq in stats["freq"].items():
            print(f"'{char}': {freq}")