-- Lists every window of every process with its buttons, two levels deep: process, window, button title, position and size.
on describe(e, depth)
	set out to ""
	tell application "System Events"
		try
			repeat with b in (buttons of e)
				set {x, y} to position of b
				set {w, h} to size of b
				set out to out & tab & "button " & quoted form of ((name of b) as text) & " desc " & quoted form of ((description of b) as text) & " at " & x & "," & y & " size " & w & "x" & h & linefeed
			end repeat
		end try
		if depth < 3 then
			try
				repeat with g in (UI elements of e)
					set r to role of g
					if r is in {"AXGroup", "AXSheet", "AXScrollArea", "AXSplitGroup", "AXLayoutArea"} then set out to out & my describe(g, depth + 1)
				end repeat
			end try
		end if
	end tell
	return out
end describe

set out to ""
tell application "System Events"
	repeat with p in every process
		try
			if (count of windows of p) > 0 then
				set out to out & "process " & (name of p) & " frontmost " & (frontmost of p) & " background " & (background only of p) & linefeed
				repeat with w in windows of p
					try
						set {x, y} to position of w
						set {ww, hh} to size of w
						set out to out & " window " & quoted form of ((name of w) as text) & " role " & ((subrole of w) as text) & " at " & x & "," & y & " size " & ww & "x" & hh & linefeed
						set out to out & my describe(w, 1)
					end try
				end repeat
			end if
		end try
	end repeat
end tell
return out
