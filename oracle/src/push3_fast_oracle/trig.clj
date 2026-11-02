(ns push3-fast-oracle.trig
  "Writes `x sin cos tan` as hex bit patterns for random doubles, using java.lang.Math, and counts
  how often Math differs from StrictMath (fdlibm). Usage: ... trig <n> <out.txt>"
  (:require [clojure.java.io :as io*]))

(defn- bits [x] (format "%016x" (Double/doubleToRawLongBits (double x))))

(defn -main [n out]
  (let [r (java.util.Random. 42)
        n (parse-long n)
        diff (atom {:sin 0 :cos 0 :tan 0})]
    (with-open [w (io*/writer out)]
      (dotimes [_ n]
        (let [x (case (.nextInt r 3)
                  0 (* (- (.nextDouble r) 0.5) 200.0)
                  1 (* (- (.nextDouble r) 0.5) 2e12)
                  2 (* (- (.nextDouble r) 0.5) 8.0))]
          (when (not= (Math/sin x) (StrictMath/sin x)) (swap! diff update :sin inc))
          (when (not= (Math/cos x) (StrictMath/cos x)) (swap! diff update :cos inc))
          (when (not= (Math/tan x) (StrictMath/tan x)) (swap! diff update :tan inc))
          (.write w (str (bits x) " " (bits (Math/sin x)) " " (bits (Math/cos x)) " " (bits (Math/tan x)) "\n")))))
    (println "Math vs StrictMath differences in" n "samples:" @diff)))
