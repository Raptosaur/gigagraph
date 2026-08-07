package gosvc

import "strings"

func Normalize(name string) string {
	return strings.ToLower(strings.TrimSpace(name))
}
