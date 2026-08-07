package mypkg

import (
	"bufio"
	"io"
	"strings"
)

var punct = strings.NewReplacer(".", "", ",", "")

// ReadLine reads a single newline-terminated line from r.
func ReadLine(r io.Reader) (string, error) {
	br := bufio.NewReader(r)
	line, err := br.ReadString('\n')
	if err != nil {
		return "", err
	}
	return strings.TrimSpace(line), nil
}

// Normalize lowercases and strips punctuation.
func Normalize(s string) string {
	return strings.ToLower(punct.Replace(s))
}

// Classify buckets a request line.
func Classify(line string) string {
	switch Normalize(line) {
	case "ping":
		return "ping"
	case "quit", "exit":
		return "quit"
	}
	for i := 0; i < len(line); i++ {
		if line[i] == '?' {
			return "question"
		}
	}
	return "other"
}
