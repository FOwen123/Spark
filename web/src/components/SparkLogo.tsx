export function SparkLogo({ size = 24, className = "" }: { size?: number; className?: string }) {
  return (
    <span
      role="img"
      aria-label="Spark logo"
      className={`inline-block shrink-0 bg-current ${className}`}
      style={{ width: size, height: size, mask: "url(/brand/spark.svg) center / contain no-repeat" }}
    />
  );
}
