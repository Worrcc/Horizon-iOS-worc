Original armour geometry fixture. No Minecraft or mod assets are included. The 64×32
texture is a generated cyan checker with a gold horn region; the helmet has inflation
1 and the horn is a mirrored cube on a rotated child bone.

Pass this directory to `horizon-client --dev-mod` with prepared vanilla resources.
Use fresh scratch directories for `HORIZON_HOME`, `HORIZON_LAUNCHER_HOME`,
`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and set `HORIZON_LAUNCHER_NO_SELF_UPDATE=1`
and `HORIZON_NO_AUDIO=1`, as AGENTS.md requires. For example, after launching a
creative world and clearing a platform in a loaded chunk:

```mcfunction
summon minecraft:armor_stand ~ ~ ~3 {ShowArms:1b,equipment:{head:{id:"armor_fixture:horn_helmet",count:1}}}
summon minecraft:armor_stand ~2 ~ ~3 {Small:1b,ShowArms:1b,Pose:{Head:[0f,30f,15f]},equipment:{head:{id:"armor_fixture:horn_helmet",count:1}}}
item replace entity @s armor.head with armor_fixture:horn_helmet
```

The helmet should cover the head with a cyan cube and a gold horn extending above
it. Third-person player rendering uses the same model. First person has no armour
hand layer. The Rust tests include this geometry directly and need no game resources.
