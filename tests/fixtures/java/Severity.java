package tools;

public enum Severity {
    LOW,
    HIGH;

    public String tag() {
        String base = name();
        return Format.wrap(base, ordinal());
    }

    static class Format {
        static String wrap(String s, int rank) {
            return "[" + s + ":" + rank + "]";
        }
    }
}
