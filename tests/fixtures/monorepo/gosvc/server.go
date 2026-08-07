package gosvc

import "fmt"

func Greet(name string) string {
	clean := Normalize(name)
	return fmt.Sprintf("hello %s", clean)
}
