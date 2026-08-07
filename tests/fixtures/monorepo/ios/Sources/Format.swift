import Foundation

func formatScore(_ score: Int) -> String {
    if score > 100 {
        return "max"
    }
    return String(score)
}
