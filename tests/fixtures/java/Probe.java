package tools;

public class Probe {
    public static String describeRuntime() {
        String vendor = System.getProperty("java.vendor");
        return "java/" + vendor;
    }

    public static Sample makeSample() {
        return new Probe.Sample(1, 10);
    }

    public record Sample(int lo, int hi) {
        public Sample {
            if (lo > hi) {
                throw new IllegalArgumentException("lo > hi");
            }
        }

        public int width() {
            return hi - lo;
        }
    }
}
