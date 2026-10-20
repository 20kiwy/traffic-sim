#!/bin/bash
# $1 = output name, $2 = south $3 = west $4 = north $5 = east
Q="[out:json][timeout:90];(
way[\"highway\"~\"^(motorway|trunk|primary|secondary|tertiary|residential|unclassified|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link)$\"]($2,$3,$4,$5);
way[\"highway\"~\"^(footway|cycleway|path|pedestrian|steps|living_street|service)$\"][\"foot\"!=\"no\"]($2,$3,$4,$5);
node[\"highway\"~\"^(traffic_signals|crossing|bus_stop)$\"]($2,$3,$4,$5);
way[\"busway\"]($2,$3,$4,$5);
way[\"route\"=\"bus\"][\"network\"~\"DART\"]($2,$3,$4,$5);
way[\"public_transport\"]($2,$3,$4,$5);
relation[\"route\"=\"bus\"]($2,$3,$4,$5);
way[\"waterway\"]($2,$3,$4,$5);
way[\"landuse\"~\"^(residential|commercial|industrial|retail)$\"]($2,$3,$4,$5);
);out body;>;out skel qt;"
curl -s -o "$1.json" --data-urlencode "data=$Q" https://overpass-api.de/api/interpreter
echo "$1: $(stat -c%s $1.json) bytes"
