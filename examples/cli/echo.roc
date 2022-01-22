app "echo"
    packages { pf: "platform" }
    imports [ pf.Stdin, pf.Stdout, pf.Task ]
    provides [ main ] to pf

main : Task.Task {} []
main = 
    _ <- Task.await (Stdout.line "Shout into this cave and hear the echo!")
    Task.loop {} (\{} -> Task.map tick Step)
    # Task.forever tick # still does not work; loops for a while, then stack overflows for me

tick : Task.Task {} []
tick =
    shout <- Task.await Stdin.line
    Stdout.line shout
